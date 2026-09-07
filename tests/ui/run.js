// Runs every UI harness in this folder and fails the lot if any one fails.
//
// The harnesses run ui/*.js under a hand written DOM shim in Node, because
// the real UI needs a WebView2 window and this build machine cannot drive
// one. They check wiring and rendering, not pixels.
//
//   node tests/ui/run.js
//
// Each harness is a separate process: they install their own globals and
// call process.exit, so sharing one process would let the first one
// finished decide the result.
const { spawnSync } = require("child_process");
const path = require("path");

const harnesses = [
  "overlay-harness.js",
  "settings-harness.js",
  "review-harness.js",
];

let failed = 0;
for (const name of harnesses) {
  console.log("=== " + name);
  const result = spawnSync(process.execPath, [path.join(__dirname, name)], {
    stdio: "inherit",
  });
  const code = result.status === null ? 1 : result.status;
  if (code !== 0) {
    failed += 1;
    console.log("=== " + name + " FAILED (exit " + code + ")");
  }
  console.log("");
}

if (failed > 0) {
  console.log(failed + " of " + harnesses.length + " harnesses failed");
  process.exit(1);
}
console.log("all " + harnesses.length + " UI harnesses passed");
