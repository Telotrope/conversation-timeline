// Turns an address the backend handed back into one the browser can fetch.
//
// The local server hands back paths of its own (`/_dev/local-storage/...`),
// which need the API's address in front. On AWS, upload and download
// addresses are S3's presigned ones, already complete (`https://...`), and
// putting anything in front of them breaks them (migration plan §V2e, E4).

export function resolveUrl(apiBase, url){
  if(/^https?:\/\//i.test(url)) return url;
  if(url.startsWith('/')) return `${apiBase}${url}`;
  throw new Error(`the server sent an address the page can't use: ${JSON.stringify(url)}`);
}
