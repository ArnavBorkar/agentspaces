"use strict";

const assert = require("node:assert/strict");
const crypto = require("node:crypto");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");

const {
  assetName,
  binaryPath,
  ensureAsp,
  parseChecksum,
  releaseUrl,
  sha256File,
  targetTriple,
  verifyChecksum,
  versionTag,
} = require("../lib/install");

function tempDir() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "asp-npm-test-"));
}

test("maps supported npm platforms to release targets", () => {
  assert.equal(targetTriple("darwin", "arm64"), "aarch64-apple-darwin");
  assert.equal(targetTriple("darwin", "x64"), "x86_64-apple-darwin");
  assert.equal(targetTriple("linux", "arm64"), "aarch64-unknown-linux-musl");
  assert.equal(targetTriple("linux", "x64"), "x86_64-unknown-linux-musl");
  assert.throws(() => targetTriple("win32", "x64"), /unsupported platform/);
});

test("builds release asset names and URLs", () => {
  const target = "x86_64-unknown-linux-musl";

  assert.equal(versionTag("0.1.1"), "v0.1.1");
  assert.equal(versionTag("v0.1.1"), "v0.1.1");
  assert.equal(assetName("0.1.1", target), "asp-v0.1.1-x86_64-unknown-linux-musl.tar.gz");
  assert.equal(
    releaseUrl("ArnavBorkar/agentspaces", "0.1.1", target),
    "https://github.com/ArnavBorkar/agentspaces/releases/download/v0.1.1/asp-v0.1.1-x86_64-unknown-linux-musl.tar.gz"
  );
});

test("parses and verifies SHA-256 files", () => {
  const tmp = tempDir();
  try {
    const archive = path.join(tmp, "archive.tar.gz");
    const checksum = path.join(tmp, "archive.tar.gz.sha256");
    fs.writeFileSync(archive, "archive bytes");
    const digest = sha256File(archive);

    assert.equal(parseChecksum(`${digest}  archive.tar.gz\n`), digest);
    fs.writeFileSync(checksum, `${digest}  archive.tar.gz\n`);
    verifyChecksum(archive, checksum);

    fs.writeFileSync(checksum, `${"0".repeat(64)}  archive.tar.gz\n`);
    assert.throws(() => verifyChecksum(archive, checksum), /checksum mismatch/);
    assert.throws(() => parseChecksum("not-a-digest"), /valid SHA-256/);
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
});

test("ensureAsp downloads, verifies, extracts, caches, and reuses binary", async () => {
  const tmp = tempDir();
  try {
    const payload = Buffer.from("pretend tarball bytes");
    const digest = crypto.createHash("sha256").update(payload).digest("hex");
    const seen = [];

    async function fakeDownload(url, dest) {
      seen.push(path.basename(url));
      if (url.endsWith(".sha256")) {
        fs.writeFileSync(dest, `${digest}  archive.tar.gz\n`);
      } else {
        fs.writeFileSync(dest, payload);
      }
    }

    async function fakeExtract(archive, dest) {
      assert.deepEqual(fs.readFileSync(archive), payload);
      fs.mkdirSync(dest, { recursive: true });
      fs.writeFileSync(path.join(dest, "asp"), "#!/bin/sh\nexit 0\n");
    }

    const bin = await ensureAsp({
      arch: "x64",
      cacheRoot: tmp,
      downloadFile: fakeDownload,
      extractTarball: fakeExtract,
      platform: "linux",
      version: "0.1.1",
    });

    assert.equal(
      bin,
      binaryPath(tmp, "0.1.1", "x86_64-unknown-linux-musl")
    );
    assert.ok(fs.existsSync(bin));
    assert.deepEqual(seen, [
      "asp-v0.1.1-x86_64-unknown-linux-musl.tar.gz",
      "asp-v0.1.1-x86_64-unknown-linux-musl.tar.gz.sha256",
    ]);

    const reused = await ensureAsp({
      arch: "x64",
      cacheRoot: tmp,
      downloadFile: async () => {
        throw new Error("download should not run for cached binary");
      },
      platform: "linux",
      version: "0.1.1",
    });

    assert.equal(reused, bin);
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
});
