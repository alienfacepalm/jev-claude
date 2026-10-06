// The tagged JSON encoding the case files use for JavaScript values that plain JSON cannot hold.
// It is documented in conformance/cases/README.md; ports decode it before calling their own code.

const TAGS = new Set(["$undefined", "$number", "$utf16", "$hex", "$throws", "$clock"]);

const LONE_SURROGATE = /[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/;

export const hasLoneSurrogate = (text) => LONE_SURROGATE.test(text);

/** Marks a value already in tagged form, so `encode` passes it through instead of rejecting it. */
class Tagged {
  constructor(value) {
    this.value = value;
  }
}

/** The expected value of a call that threw: ports must fail the same call (the message is free-form). */
export const THROWS = new Tagged({ $throws: true });

/** An `at` the status store took from the clock, which a port matches with any number. */
export const CLOCK = new Tagged({ $clock: true });

/** Calls `fn`, returning its result or THROWS. */
export function attempt(fn) {
  try {
    return fn();
  } catch {
    return THROWS;
  }
}

export function encode(value) {
  if (value instanceof Tagged) return value.value;
  if (value === undefined) return { $undefined: true };
  if (value === null || typeof value === "boolean") return value;
  if (typeof value === "number") {
    if (Number.isNaN(value)) return { $number: "NaN" };
    if (value === Infinity) return { $number: "Infinity" };
    if (value === -Infinity) return { $number: "-Infinity" };
    if (Object.is(value, -0)) return { $number: "-0" };
    return value;
  }
  if (typeof value === "string") {
    if (!hasLoneSurrogate(value)) return value;
    return { $utf16: Array.from({ length: value.length }, (_, i) => value.charCodeAt(i)) };
  }
  if (Buffer.isBuffer(value)) return { $hex: value.toString("hex") };
  if (Array.isArray(value)) {
    for (let i = 0; i < value.length; i++) if (!(i in value)) throw new Error("sparse arrays are not encodable");
    return value.map(encode);
  }
  if (typeof value === "object" && Object.getPrototypeOf(value) === Object.prototype) {
    const keys = Object.keys(value);
    // A real object shaped like a tag would decode as the tag; refuse rather than corrupt it.
    if (keys.length === 1 && TAGS.has(keys[0])) throw new Error(`input collides with tag ${keys[0]}`);
    const out = {};
    for (const key of keys) {
      if (hasLoneSurrogate(key)) throw new Error("object keys with lone surrogates are not encodable");
      out[key] = encode(value[key]);
    }
    return out;
  }
  throw new Error(`not encodable: ${Object.prototype.toString.call(value)}`);
}
