import test from "node:test";
import assert from "node:assert";
import {
  validateMqttPairCode,
  isValidBridgePort,
  isValidRelayUrl,
} from "../src/settings/esp32.ts";

test("isValidBridgePort validates ports 1024..=65535", () => {
  assert.strictEqual(isValidBridgePort(1024), true);
  assert.strictEqual(isValidBridgePort(8787), true);
  assert.strictEqual(isValidBridgePort(65535), true);
  assert.strictEqual(isValidBridgePort(1023), false);
  assert.strictEqual(isValidBridgePort(65536), false);
  assert.strictEqual(isValidBridgePort(80), false);
});

test("isValidRelayUrl validates URLs", () => {
  assert.strictEqual(isValidRelayUrl("https://relay.example.com"), true);
  assert.strictEqual(isValidRelayUrl("http://127.0.0.1:8788"), true);
  assert.strictEqual(isValidRelayUrl("http://localhost"), true);
  assert.strictEqual(isValidRelayUrl(""), false);
  assert.strictEqual(isValidRelayUrl("ftp://example.com"), false);
  assert.strictEqual(isValidRelayUrl("http://example.com"), false);
});

test("validateMqttPairCode accepts valid QR code payload", () => {
  const valid = JSON.stringify({
    t: "sm-mqtt",
    v: 1,
    uri: "mqtts://broker.hivemq.com:8883",
    u: "user1",
    p: "secret123",
    sala: "abc123def456",
    k: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  });
  const res = validateMqttPairCode(valid);
  assert.strictEqual(res.ok, true);
  if (res.ok) {
    assert.strictEqual(res.host, "broker.hivemq.com");
    assert.strictEqual(res.room, "abc123def456");
  }
});

test("validateMqttPairCode rejects invalid type 't' or version 'v'", () => {
  const base = {
    t: "sm-mqtt",
    v: 1,
    uri: "mqtts://broker.hivemq.com:8883",
    u: "user1",
    p: "secret123",
    sala: "abc123def456",
    k: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  };

  assert.strictEqual(validateMqttPairCode(JSON.stringify({ ...base, t: "sm-ble" })).ok, false);
  assert.strictEqual(validateMqttPairCode(JSON.stringify({ ...base, v: 2 })).ok, false);
  assert.strictEqual(validateMqttPairCode(JSON.stringify({ ...base, v: 0 })).ok, false);
});

test("validateMqttPairCode rejects URI without mqtts:// or invalid host", () => {
  const base = {
    t: "sm-mqtt",
    v: 1,
    uri: "mqtts://broker.hivemq.com:8883",
    u: "user1",
    p: "secret123",
    sala: "abc123def456",
    k: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  };

  assert.strictEqual(validateMqttPairCode(JSON.stringify({ ...base, uri: "mqtt://broker.hivemq.com:1883" })).ok, false);
  assert.strictEqual(validateMqttPairCode(JSON.stringify({ ...base, uri: "https://broker.hivemq.com" })).ok, false);
  assert.strictEqual(validateMqttPairCode(JSON.stringify({ ...base, uri: "mqtts://" })).ok, false);
  assert.strictEqual(validateMqttPairCode(JSON.stringify({ ...base, uri: "not-a-url" })).ok, false);
});

test("validateMqttPairCode rejects invalid sala format", () => {
  const base = {
    t: "sm-mqtt",
    v: 1,
    uri: "mqtts://broker.hivemq.com:8883",
    u: "user1",
    p: "secret123",
    sala: "abc123def456",
    k: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  };

  assert.strictEqual(validateMqttPairCode(JSON.stringify({ ...base, sala: "abc123" })).ok, false); // < 12
  assert.strictEqual(validateMqttPairCode(JSON.stringify({ ...base, sala: "abc123def4567" })).ok, false); // > 12
  assert.strictEqual(validateMqttPairCode(JSON.stringify({ ...base, sala: "ABC123DEF456" })).ok, false); // uppercase
  assert.strictEqual(validateMqttPairCode(JSON.stringify({ ...base, sala: "abc-123_456!" })).ok, false); // non-alphanumeric
});

test("validateMqttPairCode rejects invalid key 'k'", () => {
  const base = {
    t: "sm-mqtt",
    v: 1,
    uri: "mqtts://broker.hivemq.com:8883",
    u: "user1",
    p: "secret123",
    sala: "abc123def456",
    k: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  };

  assert.strictEqual(validateMqttPairCode(JSON.stringify({ ...base, k: "short" })).ok, false);
  assert.strictEqual(validateMqttPairCode(JSON.stringify({ ...base, k: "g".repeat(64) })).ok, false); // non-hex
  assert.strictEqual(validateMqttPairCode(JSON.stringify({ ...base, k: "0".repeat(63) })).ok, false);
  assert.strictEqual(validateMqttPairCode(JSON.stringify({ ...base, k: "0".repeat(65) })).ok, false);

  // uppercase hex is valid
  const upperKey = "0123456789ABCDEF".repeat(4);
  assert.strictEqual(validateMqttPairCode(JSON.stringify({ ...base, k: upperKey })).ok, true);
});

test("validateMqttPairCode rejects empty strings, invalid JSON or missing credentials", () => {
  const base = {
    t: "sm-mqtt",
    v: 1,
    uri: "mqtts://broker.hivemq.com:8883",
    u: "user1",
    p: "secret123",
    sala: "abc123def456",
    k: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  };

  assert.strictEqual(validateMqttPairCode("").ok, false);
  assert.strictEqual(validateMqttPairCode("   ").ok, false);
  assert.strictEqual(validateMqttPairCode("not-json").ok, false);
  assert.strictEqual(validateMqttPairCode("123").ok, false);
  assert.strictEqual(validateMqttPairCode("null").ok, false);
  assert.strictEqual(validateMqttPairCode("[]").ok, false);
  assert.strictEqual(validateMqttPairCode(JSON.stringify({ ...base, u: "" })).ok, false);
  assert.strictEqual(validateMqttPairCode(JSON.stringify({ ...base, p: "" })).ok, false);
});
