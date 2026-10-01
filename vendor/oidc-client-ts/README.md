# oidc-client-ts 3.5.0 (vendored)

`oidc-client-ts.min.js` is the prebuilt browser file from the npm package `oidc-client-ts` version
3.5.0 (`dist/browser/oidc-client-ts.min.js`), unchanged. It defines one global, `oidc`, and needs
no build step, which is why it's kept here rather than installed: the page has no build step.
`frontend/infra/cognito-login.js` uses it for the Cognito sign-in (migration plan §V2e, E5).

SHA-256: `f999daa383f09eb090d0ae32211e9ca931c76840e08e47fcf334fc8f5f74f1ef`

Licenses:

- `oidc-client-ts`: Apache-2.0, [LICENSE](LICENSE).
- It bundles `jwt-decode` 4.0.0: MIT, [LICENSE-jwt-decode](LICENSE-jwt-decode).

To update: `npm pack oidc-client-ts@<version>`, copy `package/dist/browser/oidc-client-ts.min.js`
and `package/LICENSE` here, check which `jwt-decode` version its `package.json` names and copy that
license too, then update the version and checksum above.
