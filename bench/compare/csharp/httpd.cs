// Concurrency: an HTTP/1.1 server on loopback (System.Net.HttpListener, async accept, a task per
// request), every request answered `200 hello` as text; serves on the address its argument names
// (`127.0.0.1:8083`) until killed.
using System.Net;
using System.Text;

var l = new HttpListener();
l.Prefixes.Add("http://" + args[0] + "/");
l.Start();
var body = Encoding.UTF8.GetBytes("hello");
while (true)
{
    var ctx = await l.GetContextAsync();
    _ = Task.Run(() =>
    {
        ctx.Response.ContentType = "text/plain; charset=utf-8";
        ctx.Response.ContentLength64 = body.Length;
        ctx.Response.OutputStream.Write(body, 0, body.Length);
        ctx.Response.Close();
    });
}
