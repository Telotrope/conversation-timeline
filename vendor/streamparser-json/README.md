# @streamparser/json 0.0.26 (vendored)

These files are the ES-module build (`dist/mjs/**/*.js`) of the npm package `@streamparser/json`
version 0.0.26, in the same folders, so their own relative imports still work. One change: each
file's last line, a `//# sourceMappingURL=…` comment naming a source map, is removed. The maps (and
the TypeScript they point to) aren't vendored, and Node's test-coverage report stops with an error
when a file names a map it can't read. They need
no build step, which is why they're kept here rather than installed: the page has no build step.
`frontend/workers/slim-stream.js` imports `index.js` to read a chosen export one conversation at a
time in a Web Worker (plan `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §7b). The
package's type declarations and source maps are left out.

SHA-256 of the `.js` files, listed in sorted order and hashed together
(`find . -name '*.js' | sort | xargs sha256sum | sha256sum`), after that change:
`48958f15363e97955351c40d70cb98878678f599f38a3670f26f61f28d8c784a`

License: MIT, [LICENSE](LICENSE) (copied from the package).

To update: `npm pack @streamparser/json@<version>`, copy every `.js` file under
`package/dist/mjs/` here keeping its folder, copy `package/LICENSE`, then update the version and
checksum above.
