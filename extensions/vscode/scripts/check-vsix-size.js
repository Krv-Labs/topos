#!/usr/bin/env node

const fs = require("fs");
const path = require("path");

const DEFAULT_LIMIT_BYTES = 200 * 1024 * 1024;
const limitBytes = Number.parseInt(process.env.TOPOS_VSIX_SIZE_LIMIT_BYTES || `${DEFAULT_LIMIT_BYTES}`, 10);
const vsixPath = process.argv[2];

if (!vsixPath) {
  console.error("Usage: node scripts/check-vsix-size.js <path-to-vsix>");
  process.exit(2);
}

const absolutePath = path.resolve(vsixPath);
const sizeBytes = fs.statSync(absolutePath).size;
const sizeMiB = sizeBytes / 1024 / 1024;
const limitMiB = limitBytes / 1024 / 1024;

console.log(`${absolutePath}: ${sizeMiB.toFixed(2)} MiB`);

if (sizeBytes > limitBytes) {
  console.error(`VSIX exceeds configured limit (${limitMiB.toFixed(2)} MiB).`);
  process.exit(1);
}
