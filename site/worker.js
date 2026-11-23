// Thin front for the static site: redirects retired consultancy URLs,
// counts page views server-side (no cookies), then serves the static build.
const RETIRED = [/^\/notes(\/|$)/, /^\/services\/?$/, /^\/examples\/?$/, /^\/catalogue\/?$/,
  /^\/zeiss-inspect-automation\/?$/, /^\/thanks\/?$/];

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (url.hostname === "www.metrologymaven.io") {
      url.hostname = "metrologymaven.io";
      return Response.redirect(url.toString(), 301);
    }
    if (RETIRED.some((re) => re.test(url.pathname))) {
      return Response.redirect(`${url.origin}/`, 301);
    }
    const res = await env.ASSETS.fetch(request);
    try {
      const type = res.headers.get("content-type") || "";
      if (request.method === "GET" && type.startsWith("text/html") && env.SITE_AE) {
        const ref = request.headers.get("referer");
        env.SITE_AE.writeDataPoint({
          blobs: [url.pathname, request.cf?.country || "", ref ? new URL(ref).hostname : ""],
          doubles: [res.status],
          indexes: [url.pathname.slice(0, 96)],
        });
      }
    } catch (_) { /* analytics must never break a page */ }
    const out = new Response(res.body, res);
    out.headers.set("X-Content-Type-Options", "nosniff");
    out.headers.set("Referrer-Policy", "strict-origin-when-cross-origin");
    const isGuide = url.pathname === "/guide" || url.pathname.startsWith("/guide/");
    const scriptSrc = isGuide ? "'self' 'wasm-unsafe-eval'" : "'none'";
    out.headers.set("Content-Security-Policy", `default-src 'self'; img-src 'self' data:; style-src 'self'; script-src ${scriptSrc}; frame-ancestors 'none'; base-uri 'none'`);
    return out;
  },
};
