#!/usr/bin/env node
"use strict";

const childProcess = require("node:child_process");
const { AspInstallError, ensureAsp } = require("../lib/install");

async function main() {
  const asp = await ensureAsp();
  const result = childProcess.spawnSync(asp, process.argv.slice(2), {
    stdio: "inherit",
  });

  if (result.error) {
    throw new AspInstallError(
      `failed to run asp: ${result.error.message}`,
      "check that the cached binary is executable, or delete the npm cache entry and retry"
    );
  }

  if (result.signal) {
    process.kill(process.pid, result.signal);
    return;
  }

  process.exit(result.status === null ? 1 : result.status);
}

main().catch((err) => {
  console.error(`error: ${err.message}`);
  if (err.hint) {
    console.error(`hint: ${err.hint}`);
  }
  process.exit(1);
});
