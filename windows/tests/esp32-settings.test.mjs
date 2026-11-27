import test from "node:test";
import assert from "node:assert";
import {
  isValidPairCode,
  isValidBridgePort,
  isValidRoomId,
  isValidRelayUrl,
} from "../src/settings/esp32.ts";

test("isValidPairCode validates 6 digits", () => {
  assert.strictEqual(isValidPairCode("123456"), true);
  assert.strictEqual(isValidPairCode("000000"), true);
  assert.strictEqual(isValidPairCode("12345"), false);
  assert.strictEqual(isValidPairCode("1234567"), false);
  assert.strictEqual(isValidPairCode("abcdef"), false);
  assert.strictEqual(isValidPairCode("12345a"), false);
});

test("isValidBridgePort validates ports 1024..=65535", () => {
  assert.strictEqual(isValidBridgePort(1024), true);
  assert.strictEqual(isValidBridgePort(8787), true);
  assert.strictEqual(isValidBridgePort(65535), true);
  assert.strictEqual(isValidBridgePort(1023), false);
  assert.strictEqual(isValidBridgePort(65536), false);
  assert.strictEqual(isValidBridgePort(80), false);
});

test("isValidRoomId validates room format", () => {
  assert.strictEqual(isValidRoomId("room-1234_ABCD"), true);
  assert.strictEqual(isValidRoomId("short"), false); // < 8 chars
  assert.strictEqual(isValidRoomId("a".repeat(65)), false); // > 64 chars
  assert.strictEqual(isValidRoomId("room with spaces"), false);
});

test("isValidRelayUrl validates URLs", () => {
  assert.strictEqual(isValidRelayUrl("https://relay.example.com"), true);
  assert.strictEqual(isValidRelayUrl("http://127.0.0.1:8788"), true);
  assert.strictEqual(isValidRelayUrl("http://localhost"), true);
  assert.strictEqual(isValidRelayUrl(""), false);
  assert.strictEqual(isValidRelayUrl("ftp://example.com"), false);
  assert.strictEqual(isValidRelayUrl("http://example.com"), false);
});
