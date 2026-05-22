#!/usr/bin/env node

const fs = require("fs");
const path = require("path");
const { execFileSync } = require("child_process");

const TARGETS = {
  "darwin-arm64": "macos-arm64",
  "darwin-x64": "macos-amd64",
  "linux-arm64": "linux-arm64",
  "linux-x64": "linux-amd64",
};

const extensionRoot = path.resolve(__dirname, "..");
const repoRoot = path.resolve(extensionRoot, "..", "..");
const binDir = path.join(extensionRoot, "bin");
const stagedBinary = path.join(binDir, "topos");

function usage() {
  console.error("Usage: node scripts/stage-binary.js <darwin-arm64|darwin-x64|linux-arm64|linux-x64|clean>");
}

function clean() {
  fs.rmSync(binDir, { recursive: true, force: true });
}

function candidatePaths(target) {
  const artifactName = TARGETS[target];
  const explicitSource = process.env.TOPOS_BINARY_SOURCE;
  return [
    explicitSource,
    path.join(repoRoot, "dist", `topos-${artifactName}`),
    path.join(repoRoot, "dist", artifactName, `topos-${artifactName}`),
    path.join(repoRoot, "dist", `topos-${target}`),
  ].filter(Boolean);
}

function findSource(target) {
  const candidates = candidatePaths(target);
  const found = candidates.find((candidate) => {
    try {
      return fs.statSync(candidate).isFile();
    } catch {
      return false;
    }
  });

  if (!found) {
    throw new Error(`No Topos binary found for ${target}. Checked:\n${candidates.map((candidate) => `  - ${candidate}`).join("\n")}`);
  }

  return found;
}

function verifyDarwinSignature(target, filePath) {
  if (!target.startsWith("darwin-")) return;
  if (process.env.TOPOS_REQUIRE_DARWIN_CODESIGN !== "1") return;

  execFileSync("codesign", ["--verify", "--strict", "--verbose=2", filePath], {
    stdio: "inherit",
  });
}

function stage(target) {
  if (!TARGETS[target]) {
    usage();
    process.exitCode = 2;
    return;
  }

  const source = findSource(target);
  clean();
  fs.mkdirSync(binDir, { recursive: true });
  fs.copyFileSync(source, stagedBinary);
  fs.chmodSync(stagedBinary, 0o755);
  verifyDarwinSignature(target, stagedBinary);

  const sizeBytes = fs.statSync(stagedBinary).size;
  if (sizeBytes <= 0) {
    throw new Error(`Staged binary is empty: ${stagedBinary}`);
  }

  console.log(`Staged ${target} runtime: ${source} -> ${stagedBinary} (${sizeBytes} bytes)`);
}

function main() {
  const target = process.argv[2];
  if (!target) {
    usage();
    process.exit(2);
  }

  if (target === "clean") {
    clean();
    return;
  }

  stage(target);
}

if (require.main === module) {
  try {
    main();
  } catch (err) {
    console.error(err.message || err);
    process.exit(1);
  }
}

module.exports = {
  TARGETS,
  candidatePaths,
  stage,
  clean,
  stagedBinary,
};
