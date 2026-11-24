// Thin front for the static site: redirects retired consultancy URLs,
// counts page views server-side (no cookies), then serves the static build.
const RETIRED = [/^\/notes(\/|$)/, /^\/services\/?$/, /^\/examples\/?$/, /^\/catalogue\/?$/,
  /^\/zeiss-inspect-automation\/?$/, /^\/thanks\/?$/];

function normalizePath(rawPathname) {
  let decoded;
  try {
    decoded = decodeURIComponent(rawPathname);
  } catch (_) {
    return null;
  }
  const parts = [];
  for (const seg of decoded.split("/")) {
    if (!seg || seg === ".") continue;
    if (seg === "..") {
      parts.pop();
      continue;
    }
    parts.push(seg);
  }
  const trailing = decoded.length > 1 && decoded.endsWith("/") ? "/" : "";
  return "/" + parts.join("/") + (parts.length && trailing ? "/" : "");
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (url.hostname === "www.metrologymaven.io") {
      url.hostname = "metrologymaven.io";
      return Response.redirect(url.toString(), 301);
    }
    const normPath = normalizePath(url.pathname);
    if (normPath !== null && RETIRED.some((re) => re.test(normPath))) {
      return Response.redirect(`${url.origin}/`, 301);
    }
    const res = await env.ASSETS.fetch(request);
    try {
      const type = res.headers.get("content-type") || "";
      if (request.method === "GET" && type.startsWith("text/html") && env.SITE_AE) {
        const ref = request.headers.get("referer");
        env.SITE_AE.writeDataPoint({
          blobs: [normPath ?? url.pathname, request.cf?.country || "", ref ? new URL(ref).hostname : ""],
          doubles: [res.status],
          indexes: [(normPath ?? url.pathname).slice(0, 96)],
        });
      }
    } catch (_) { /* analytics must never break a page */ }
    const out = new Response(res.body, res);
    out.headers.set("X-Content-Type-Options", "nosniff");
    out.headers.set("Referrer-Policy", "strict-origin-when-cross-origin");
    const isGuide = normPath !== null && (normPath === "/guide" || normPath.startsWith("/guide/"));
    const scriptSrc = isGuide ? "'self' 'wasm-unsafe-eval'" : "'none'";
    out.headers.set("Content-Security-Policy", `default-src 'self'; img-src 'self' data:; style-src 'self'; script-src ${scriptSrc}; frame-ancestors 'none'; base-uri 'none'`);
    return out;
  },
};
