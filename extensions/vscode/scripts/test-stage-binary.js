#!/usr/bin/env node

const assert = require("assert");
const fs = require("fs");
const os = require("os");
const path = require("path");
const { candidatePaths, clean, stage, stagedBinary } = require("./stage-binary");

function withTempBinary(fn) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "topos-vscode-stage-"));
  const source = path.join(dir, "topos");
  fs.writeFileSync(source, "#!/bin/sh\nexit 0\n");
  fs.chmodSync(source, 0o755);

  const previousSource = process.env.TOPOS_BINARY_SOURCE;
  process.env.TOPOS_BINARY_SOURCE = source;

  try {
    fn(source);
  } finally {
    if (previousSource === undefined) {
      delete process.env.TOPOS_BINARY_SOURCE;
    } else {
      process.env.TOPOS_BINARY_SOURCE = previousSource;
    }
    fs.rmSync(dir, { recursive: true, force: true });
    clean();
  }
}

assert(candidatePaths("linux-x64").some((candidate) => candidate.endsWith("dist/topos-linux-amd64")));
assert(candidatePaths("darwin-arm64").some((candidate) => candidate.endsWith("dist/topos-macos-arm64")));

withTempBinary(() => {
  stage("linux-x64");
  assert(fs.statSync(stagedBinary).isFile());
  assert(fs.statSync(stagedBinary).mode & 0o111);
});

console.log("stage-binary tests passed");
