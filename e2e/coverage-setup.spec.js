// Every browser test is measured: each spec file must take `test` from
// ./fixtures, whose `page` records coverage, never straight from
// @playwright/test (docs/plans/2026-10-02-browser-coverage-every-test.md).

const fs = require('fs');
const path = require('path');
const { test, expect } = require('./fixtures');

test('every spec file takes its test from ./fixtures', () => {
  const specs = fs.readdirSync(__dirname).filter((f) => f.endsWith('.spec.js'));
  expect(specs.length).toBeGreaterThan(1);
  const bypassing = specs.filter((f) => {
    const text = fs.readFileSync(path.join(__dirname, f), 'utf8');
    return /require\(['"]@playwright\/test['"]\)|from ['"]@playwright\/test['"]/.test(text)
      || !/require\(['"]\.\/fixtures['"]\)/.test(text);
  });
  expect(bypassing, 'spec files not measured').toEqual([]);
});
