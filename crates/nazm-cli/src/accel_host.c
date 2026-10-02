// Written by nazm accel (N67, docs/architecture.md §7.69): the host side of one kernel run.
// argv: kernel.cl input.bin output.bin. Every transfer and synchronisation is explicit and timed;
// one line of JSON on stdout says what happened.
#define CL_SILENCE_DEPRECATION
#include <OpenCL/opencl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

static double now_ms(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (double)t.tv_sec * 1e3 + (double)t.tv_nsec / 1e6;
}

static char *slurp(const char *path, size_t *len) {
    FILE *f = fopen(path, "rb");
    if (!f) return NULL;
    fseek(f, 0, SEEK_END);
    long n = ftell(f);
    fseek(f, 0, SEEK_SET);
    char *b = malloc((size_t)n + 1);
    if (!b || fread(b, 1, (size_t)n, f) != (size_t)n) { fclose(f); return NULL; }
    b[n] = 0;
    fclose(f);
    *len = (size_t)n;
    return b;
}

#define CHECK(what, e) if ((e) != CL_SUCCESS) { printf("{\"error\":\"%s\",\"code\":%d}\n", what, (int)(e)); return 3; }

int main(int argc, char **argv) {
    if (argc != 4) return 4;
    size_t src_len, in_len;
    char *src = slurp(argv[1], &src_len);
    char *in = slurp(argv[2], &in_len);
    if (!src || !in) { printf("{\"error\":\"unreadable input\"}\n"); return 3; }
    size_t n = in_len / 8;
    cl_int e;
    cl_platform_id p;
    CHECK("platform", clGetPlatformIDs(1, &p, NULL));
    cl_device_id d;
    CHECK("device", clGetDeviceIDs(p, CL_DEVICE_TYPE_GPU, 1, &d, NULL));
    char name[256] = {0}, driver[256] = {0}, version[256] = {0};
    clGetDeviceInfo(d, CL_DEVICE_NAME, sizeof name - 1, name, NULL);
    clGetDeviceInfo(d, CL_DRIVER_VERSION, sizeof driver - 1, driver, NULL);
    clGetDeviceInfo(d, CL_DEVICE_VERSION, sizeof version - 1, version, NULL);
    cl_context c = clCreateContext(NULL, 1, &d, NULL, NULL, &e);
    CHECK("context", e);
    cl_command_queue q = clCreateCommandQueue(c, d, 0, &e);
    CHECK("queue", e);

    double t0 = now_ms();
    const char *s = src;
    cl_program pr = clCreateProgramWithSource(c, 1, &s, &src_len, &e);
    CHECK("program", e);
    e = clBuildProgram(pr, 1, &d, "", NULL, NULL);
    if (e != CL_SUCCESS) {
        char log[8192] = {0};
        clGetProgramBuildInfo(pr, d, CL_PROGRAM_BUILD_LOG, sizeof log - 1, log, NULL);
        fprintf(stderr, "%s\n", log);
        CHECK("build", e);
    }
    cl_kernel k = clCreateKernel(pr, "nz_kernel", &e);
    CHECK("kernel", e);
    double t1 = now_ms();

    int fail = 0x7fffffff;
    cl_mem bin = clCreateBuffer(c, CL_MEM_READ_ONLY, n * 8 + 8, NULL, &e);
    CHECK("input buffer", e);
    cl_mem bout = clCreateBuffer(c, CL_MEM_WRITE_ONLY, n * 8 + 8, NULL, &e);
    CHECK("output buffer", e);
    cl_mem bfail = clCreateBuffer(c, CL_MEM_READ_WRITE, 4, NULL, &e);
    CHECK("failure buffer", e);
    // Host to device: the input and the failure word, then a synchronisation.
    CHECK("write input", clEnqueueWriteBuffer(q, bin, CL_TRUE, 0, n * 8, in, 0, NULL, NULL));
    CHECK("write failure", clEnqueueWriteBuffer(q, bfail, CL_TRUE, 0, 4, &fail, 0, NULL, NULL));
    double t2 = now_ms();
    CHECK("arg 0", clSetKernelArg(k, 0, sizeof bin, &bin));
    CHECK("arg 1", clSetKernelArg(k, 1, sizeof bout, &bout));
    CHECK("arg 2", clSetKernelArg(k, 2, sizeof bfail, &bfail));
    if (n > 0) CHECK("launch", clEnqueueNDRangeKernel(q, k, 1, NULL, &n, NULL, 0, NULL, NULL));
    // The one synchronisation point: every work item has finished before anything is read.
    CHECK("finish", clFinish(q));
    double t3 = now_ms();
    int64_t *out = malloc(n * 8 + 8);
    // Device to host: the failure word, then the results.
    CHECK("read failure", clEnqueueReadBuffer(q, bfail, CL_TRUE, 0, 4, &fail, 0, NULL, NULL));
    CHECK("read output", clEnqueueReadBuffer(q, bout, CL_TRUE, 0, n * 8, out, 0, NULL, NULL));
    double t4 = now_ms();
    FILE *o = fopen(argv[3], "wb");
    if (!o || fwrite(out, 8, n, o) != n) { printf("{\"error\":\"unwritable output\"}\n"); return 3; }
    fclose(o);
    printf("{\"device\":\"%s\",\"driver\":\"%s\",\"version\":\"%s\",\"elements\":%zu,"
           "\"to_device_bytes\":%zu,\"from_device_bytes\":%zu,"
           "\"build_ms\":%.3f,\"to_device_ms\":%.3f,\"run_ms\":%.3f,\"from_device_ms\":%.3f,\"fail\":%d}\n",
           name, driver, version, n, n * 8 + 4, n * 8 + 4, t1 - t0, t2 - t1, t3 - t2, t4 - t3, fail);
    return 0;
}
