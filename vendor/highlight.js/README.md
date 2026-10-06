# highlight.js 11.12.0 (vendored)

`highlight.min.js` is the prebuilt browser file (the common languages) from the npm package
`@highlightjs/cdn-assets` version 11.12.0, unchanged; `github.min.css` is that package's
`styles/github.min.css`, unchanged. The script defines one global, `hljs`, and needs no build step,
which is why it's kept here rather than installed: the page has no build step.
`frontend/ui/render/file-view.js` uses it to colour code in the file viewer (plan
`docs/plans/2026-10-06-load-only-what-the-page-shows.md` §4). It only colours text; it runs nothing
from the file.

SHA-256:

- `highlight.min.js`: `8ab71eb09c51f501e5e25157d9cff100e46cc29bcbfc744d0b746d451fca7f53`
- `github.min.css`: `3a9a5def8b9c311e5ae43abde85c63133185eed4f0d9f67fea4b00a8308cf066`

License: BSD-3-Clause, [LICENSE](LICENSE) (copied from the package).

To update: `npm pack @highlightjs/cdn-assets@<version>`, copy `package/highlight.min.js`,
`package/styles/github.min.css` and `package/LICENSE` here, then update the version and checksums
above.
