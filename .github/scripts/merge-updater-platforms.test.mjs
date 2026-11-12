import assert from "node:assert/strict";
import test from "node:test";
import { mergeUpdaterPlatforms } from "./merge-updater-platforms.mjs";

test("fills macOS and Windows updater platforms from signature assets", () => {
  const manifest = {
    version: "1.0.8",
    platforms: {
      "windows-x86_64": {
        signature: "stale",
        url: "https://example.invalid/setup.exe",
      },
    },
  };
  const release = {
    assets: [
      { name: "Cursor.BYOK_1.0.8_x64-setup.exe" },
      { name: "Cursor.BYOK_1.0.8_aarch64.app.tar.gz" },
      { name: "Cursor.BYOK_1.0.8_aarch64.dmg" },
      { name: "cursor-byok-1.0.8-windows-amd64.zip" },
    ],
  };

  mergeUpdaterPlatforms(
    manifest,
    release,
    {
      "Cursor.BYOK_1.0.8_x64-setup.exe.sig": "windows-signature\n",
      "Cursor.BYOK_1.0.8_aarch64.app.tar.gz.sig": "mac-signature\n",
      "cursor-byok-1.0.8-windows-amd64.zip.sig": "legacy-signature\n",
    },
    "owner/repository",
    "1.0.8",
  );

  assert.equal(manifest.platforms["windows-x86_64"].signature, "windows-signature");
  assert.equal(manifest.platforms["windows-x86_64-nsis"].signature, "windows-signature");
  assert.equal(
    manifest.platforms["darwin-aarch64"].url,
    "https://github.com/owner/repository/releases/download/v1.0.8/Cursor.BYOK_1.0.8_aarch64.app.tar.gz",
  );
  assert.equal(manifest.platforms["darwin-aarch64-app"].signature, "mac-signature");
  assert.equal(manifest.platforms["windows-amd64"], undefined);
});
