// Downloads the engine asks for (upstream's launch:DownloadPage, queued by
// crates/session instead of run): fetched here and handed back with
// `downloadDone`. Requests for pathofexile.com go through the proxy set in
// the site's config.json (crates/proxy), since GGG's site sends no CORS
// headers; anything else is fetched directly.

export type Download = { id: number; url: string; header?: string; body?: string };

export type DownloadResult = { id: number; status?: number; body?: string; header?: string; error?: string };

type SiteConfig = { poeProxy?: string };

let config: Promise<SiteConfig> | null = null;

// config.json next to the page, optional: {"poeProxy": "https://proxy.example.com"}
export function siteConfig(): Promise<SiteConfig> {
  config ??= fetch(new URL("config.json", document.baseURI))
    .then((r) => (r.ok ? r.json() : {}))
    .catch(() => ({}));
  return config;
}

const GGG = /^https:\/\/www\.pathofexile\.com\//;

export async function fetchDownload(d: Download): Promise<DownloadResult> {
  let url = d.url;
  if (GGG.test(url)) {
    const { poeProxy } = await siteConfig();
    if (!poeProxy) {
      return { id: d.id, error: "no proxy for pathofexile.com is configured (poeProxy in config.json)" };
    }
    url = poeProxy.replace(/\/$/, "") + "/" + url.replace(GGG, "");
  }
  try {
    const headers: Record<string, string> = {};
    for (const line of (d.header ?? "").split(/\r?\n/)) {
      const m = line.match(/^([^:]+):\s*(.*)$/);
      if (m) headers[m[1]] = m[2];
    }
    const r = await fetch(url, d.body !== undefined ? { method: "POST", headers, body: d.body } : { headers });
    let header = "";
    r.headers.forEach((v, k) => (header += `${k}: ${v}\r\n`));
    return { id: d.id, status: r.status, body: await r.text(), header };
  } catch (e) {
    return { id: d.id, error: String(e instanceof Error ? e.message : e) };
  }
}
