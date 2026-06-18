"use strict";

const childProcess = require("node:child_process");
const crypto = require("node:crypto");
const fs = require("node:fs");
const http = require("node:http");
const https = require("node:https");
const os = require("node:os");
const path = require("node:path");

const PACKAGE = require("../package.json");
const DEFAULT_REPO = "ArnavBorkar/agentspaces";

class AspInstallError extends Error {
  constructor(message, hint) {
    super(message);
    this.name = "AspInstallError";
    this.hint = hint;
  }
}

function versionTag(version) {
  return version.startsWith("v") ? version : `v${version}`;
}

function targetTriple(platform = process.platform, arch = process.arch) {
  if (platform === "darwin" && arch === "arm64") {
    return "aarch64-apple-darwin";
  }
  if (platform === "darwin" && arch === "x64") {
    return "x86_64-apple-darwin";
  }
  if (platform === "linux" && arch === "arm64") {
    return "aarch64-unknown-linux-musl";
  }
  if (platform === "linux" && arch === "x64") {
    return "x86_64-unknown-linux-musl";
  }

  throw new AspInstallError(
    `unsupported platform: ${platform}/${arch}`,
    "supported npx targets are macOS and Linux on arm64 or x64; use install.sh or cargo install for other systems"
  );
}

function assetName(version, target) {
  const tag = versionTag(version);
  return `asp-${tag}-${target}.tar.gz`;
}

function releaseUrl(repo, version, target) {
  const tag = versionTag(version);
  const asset = assetName(version, target);
  return `https://github.com/${repo}/releases/download/${tag}/${asset}`;
}

function defaultCacheRoot() {
  return path.join(os.homedir(), ".cache", "agentspaces", "asp");
}

function binaryPath(cacheRoot, version, target) {
  return path.join(cacheRoot, versionTag(version), target, "asp");
}

function parseChecksum(text) {
  const checksum = text.trim().split(/\s+/)[0] || "";
  if (!/^[a-fA-F0-9]{64}$/.test(checksum)) {
    throw new AspInstallError(
      "checksum file did not contain a valid SHA-256 digest",
      "do not run this archive; retry the install or verify the release manually"
    );
  }
  return checksum.toLowerCase();
}

function sha256File(file) {
  const hash = crypto.createHash("sha256");
  hash.update(fs.readFileSync(file));
  return hash.digest("hex");
}

function verifyChecksum(archive, checksumFile) {
  const expected = parseChecksum(fs.readFileSync(checksumFile, "utf8"));
  const actual = sha256File(archive);
  if (expected !== actual) {
    throw new AspInstallError(
      `checksum mismatch (expected ${expected}, got ${actual})`,
      "do not run this archive; retry the download, and report the release if the mismatch persists"
    );
  }
}

function downloadFile(url, dest, redirects = 0) {
  if (redirects > 5) {
    return Promise.reject(
      new AspInstallError(
        `too many redirects while downloading ${url}`,
        "check your network/proxy settings or download the release manually"
      )
    );
  }

  return new Promise((resolve, reject) => {
    const client = url.startsWith("http:") ? http : https;
    const req = client.get(
      url,
      { headers: { "User-Agent": "agentspaces-npm-installer" } },
      (res) => {
        if (
          res.statusCode >= 300 &&
          res.statusCode < 400 &&
          res.headers.location
        ) {
          res.resume();
          const nextUrl = new URL(res.headers.location, url).toString();
          downloadFile(nextUrl, dest, redirects + 1).then(resolve, reject);
          return;
        }

        if (res.statusCode !== 200) {
          res.resume();
          reject(
            new AspInstallError(
              `download failed: ${url} (HTTP ${res.statusCode})`,
              "check network/proxy settings, verify the release exists for this platform, or build from source"
            )
          );
          return;
        }

        const out = fs.createWriteStream(dest, { mode: 0o600 });
        res.pipe(out);
        out.on("finish", () => out.close(resolve));
        out.on("error", reject);
      }
    );

    req.on("error", (err) => {
      reject(
        new AspInstallError(
          `download failed: ${url}: ${err.message}`,
          "check network/proxy settings, set npm proxy config if needed, or build from source"
        )
      );
    });
  });
}

function extractTarball(archive, dest) {
  fs.mkdirSync(dest, { recursive: true });
  const result = childProcess.spawnSync("tar", ["-xzf", archive, "-C", dest], {
    encoding: "utf8",
  });

  if (result.error) {
    throw new AspInstallError(
      `tar extraction failed: ${result.error.message}`,
      "install tar, or use the install.sh/cargo install path instead"
    );
  }
  if (result.status !== 0) {
    throw new AspInstallError(
      `tar extraction failed: ${result.stderr.trim() || result.status}`,
      "retry the install, and report the release if extraction keeps failing"
    );
  }
}

async function ensureAsp(options = {}) {
  const version = options.version || process.env.ASP_NPM_VERSION || PACKAGE.version;
  const repo = options.repo || process.env.ASP_NPM_REPO || DEFAULT_REPO;
  const platform = options.platform || process.platform;
  const arch = options.arch || process.arch;
  const target = options.target || targetTriple(platform, arch);
  const cacheRoot =
    options.cacheRoot || process.env.ASP_NPM_CACHE_DIR || defaultCacheRoot();
  const bin = binaryPath(cacheRoot, version, target);

  if (fs.existsSync(bin)) {
    return bin;
  }

  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "asp-npm-"));
  const archive = path.join(tmp, assetName(version, target));
  const checksum = `${archive}.sha256`;
  const extractDir = path.join(tmp, "extract");
  const url = releaseUrl(repo, version, target);
  const downloader = options.downloadFile || downloadFile;
  const extractor = options.extractTarball || extractTarball;

  try {
    await downloader(url, archive);
    await downloader(`${url}.sha256`, checksum);
    verifyChecksum(archive, checksum);
    await extractor(archive, extractDir);

    const extracted = path.join(extractDir, "asp");
    if (!fs.existsSync(extracted)) {
      throw new AspInstallError(
        "release archive did not contain an asp binary",
        "report the release; the archive layout is not compatible with the npm wrapper"
      );
    }

    fs.mkdirSync(path.dirname(bin), { recursive: true });
    const tmpBin = `${bin}.tmp-${process.pid}-${Date.now()}`;
    fs.copyFileSync(extracted, tmpBin);
    fs.chmodSync(tmpBin, 0o755);
    fs.renameSync(tmpBin, bin);
    return bin;
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

module.exports = {
  AspInstallError,
  assetName,
  binaryPath,
  ensureAsp,
  parseChecksum,
  releaseUrl,
  sha256File,
  targetTriple,
  verifyChecksum,
  versionTag,
};
