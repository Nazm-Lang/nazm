// Written by nazm accel (N67, docs/internal/architecture.md §7.69): the host side of one kernel run.
// argv: kernel.cl output.bin input.bin [input2.bin] — one input for a map, two for a zip (N88).
// Every transfer and synchronisation is explicit and timed; one line of JSON on stdout says what
// happened. Built with NZ_SIZE, the bytes of one element, and for a float kernel (Gate 3) NZ_FLOAT,
// and for a Float64 one NZ_FP64, which the device must support.
#define CL_SILENCE_DEPRECATION
#include <OpenCL/opencl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
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
    if (argc != 4 && argc != 5) return 4;
    int zip = argc == 5;
    size_t src_len, in_len, in2_len = 0;
    char *src = slurp(argv[1], &src_len);
    char *in = slurp(argv[3], &in_len);
    char *in2 = zip ? slurp(argv[4], &in2_len) : NULL;
    if (!src || !in || (zip && (!in2 || in2_len != in_len))) { printf("{\"error\":\"unreadable input\"}\n"); return 3; }
    size_t n = in_len / NZ_SIZE;
    cl_int e;
    cl_platform_id p;
    CHECK("platform", clGetPlatformIDs(1, &p, NULL));
    cl_device_id d;
    CHECK("device", clGetDeviceIDs(p, CL_DEVICE_TYPE_GPU, 1, &d, NULL));
    char name[256] = {0}, driver[256] = {0}, version[256] = {0};
    clGetDeviceInfo(d, CL_DEVICE_NAME, sizeof name - 1, name, NULL);
    clGetDeviceInfo(d, CL_DRIVER_VERSION, sizeof driver - 1, driver, NULL);
    clGetDeviceInfo(d, CL_DEVICE_VERSION, sizeof version - 1, version, NULL);
#ifdef NZ_FP64
    // A Float64 kernel needs the device's double precision (Gate 3): refused, by name, without it.
    char extensions[8192] = {0};
    clGetDeviceInfo(d, CL_DEVICE_EXTENSIONS, sizeof extensions - 1, extensions, NULL);
    if (!strstr(extensions, "cl_khr_fp64")) {
        printf("{\"error\":\"no fp64\",\"device\":\"%s\"}\n", name);
        return 5;
    }
#endif
    cl_context c = clCreateContext(NULL, 1, &d, NULL, NULL, &e);
    CHECK("context", e);
    cl_command_queue q = clCreateCommandQueue(c, d, 0, &e);
    CHECK("queue", e);

    double t0 = now_ms();
    const char *s = src;
    cl_program pr = clCreateProgramWithSource(c, 1, &s, &src_len, &e);
    CHECK("program", e);
#ifdef NZ_FLOAT
    // Division and square root correctly rounded, as the language's are (Gate 3).
    e = clBuildProgram(pr, 1, &d, "-cl-fp32-correctly-rounded-divide-sqrt", NULL, NULL);
#else
    e = clBuildProgram(pr, 1, &d, "", NULL, NULL);
#endif
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
    cl_mem bin = clCreateBuffer(c, CL_MEM_READ_ONLY, n * NZ_SIZE + 8, NULL, &e);
    CHECK("input buffer", e);
    cl_mem bin2 = NULL;
    if (zip) {
        bin2 = clCreateBuffer(c, CL_MEM_READ_ONLY, n * NZ_SIZE + 8, NULL, &e);
        CHECK("second input buffer", e);
    }
    cl_mem bout = clCreateBuffer(c, CL_MEM_WRITE_ONLY, n * NZ_SIZE + 8, NULL, &e);
    CHECK("output buffer", e);
    cl_mem bfail = clCreateBuffer(c, CL_MEM_READ_WRITE, 4, NULL, &e);
    CHECK("failure buffer", e);
    // Host to device: the input and the failure word, then a synchronisation.
    CHECK("write input", clEnqueueWriteBuffer(q, bin, CL_TRUE, 0, n * NZ_SIZE, in, 0, NULL, NULL));
    if (zip) CHECK("write second input", clEnqueueWriteBuffer(q, bin2, CL_TRUE, 0, n * NZ_SIZE, in2, 0, NULL, NULL));
    CHECK("write failure", clEnqueueWriteBuffer(q, bfail, CL_TRUE, 0, 4, &fail, 0, NULL, NULL));
    double t2 = now_ms();
    cl_uint a = 0;
    CHECK("arg input", clSetKernelArg(k, a++, sizeof bin, &bin));
    if (zip) CHECK("arg second input", clSetKernelArg(k, a++, sizeof bin2, &bin2));
    CHECK("arg output", clSetKernelArg(k, a++, sizeof bout, &bout));
    CHECK("arg failure", clSetKernelArg(k, a++, sizeof bfail, &bfail));
    if (n > 0) CHECK("launch", clEnqueueNDRangeKernel(q, k, 1, NULL, &n, NULL, 0, NULL, NULL));
    // The one synchronisation point: every work item has finished before anything is read.
    CHECK("finish", clFinish(q));
    double t3 = now_ms();
    char *out = malloc(n * NZ_SIZE + 8);
    // Device to host: the failure word, then the results.
    CHECK("read failure", clEnqueueReadBuffer(q, bfail, CL_TRUE, 0, 4, &fail, 0, NULL, NULL));
    CHECK("read output", clEnqueueReadBuffer(q, bout, CL_TRUE, 0, n * NZ_SIZE, out, 0, NULL, NULL));
    double t4 = now_ms();
    FILE *o = fopen(argv[2], "wb");
    if (!o || fwrite(out, NZ_SIZE, n, o) != n) { printf("{\"error\":\"unwritable output\"}\n"); return 3; }
    fclose(o);
    printf("{\"device\":\"%s\",\"driver\":\"%s\",\"version\":\"%s\",\"elements\":%zu,"
           "\"to_device_bytes\":%zu,\"from_device_bytes\":%zu,"
           "\"build_ms\":%.3f,\"to_device_ms\":%.3f,\"run_ms\":%.3f,\"from_device_ms\":%.3f,\"fail\":%d}\n",
           name, driver, version, n, n * NZ_SIZE * (zip ? 2 : 1) + 4, n * NZ_SIZE + 4, t1 - t0, t2 - t1, t3 - t2, t4 - t3, fail);
    return 0;
}
