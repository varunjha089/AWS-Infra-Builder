// The CIDR arithmetic the inspector needs on the spot (proposing a free block); the Rust side (crates/core/src/cidr.rs) is the
// one that checks the design.
export interface Cidr { addr: number; prefix: number }

const mask = (prefix: number) => (prefix === 0 ? 0 : (0xffffffff << (32 - prefix)) >>> 0);

export function parseCidr(s: string): Cidr | null {
  const m = /^\s*(\d+)\.(\d+)\.(\d+)\.(\d+)\/(\d+)\s*$/.exec(s);
  if (!m) return null;
  const parts = m.slice(1, 5).map(Number);
  const prefix = Number(m[5]);
  if (parts.some((p) => p > 255) || prefix > 32) return null;
  const addr = ((parts[0] << 24) | (parts[1] << 16) | (parts[2] << 8) | parts[3]) >>> 0;
  return { addr: (addr & mask(prefix)) >>> 0, prefix };
}
export const cidrText = (c: Cidr) => `${c.addr >>> 24}.${(c.addr >>> 16) & 255}.${(c.addr >>> 8) & 255}.${c.addr & 255}/${c.prefix}`;
export const size = (c: Cidr) => 2 ** (32 - c.prefix);
const last = (c: Cidr) => (c.addr | (~mask(c.prefix) >>> 0)) >>> 0;
export const contains = (a: Cidr, b: Cidr) => b.prefix >= a.prefix && ((b.addr & mask(a.prefix)) >>> 0) === a.addr;
export const overlaps = (a: Cidr, b: Cidr) => contains(a, b) || contains(b, a);

/** the first aligned block of `prefix` inside `within` that overlaps none of `taken` */
export function nextFree(within: Cidr, taken: Cidr[], prefix: number): Cidr | null {
  if (prefix < within.prefix || prefix > 32) return null;
  const step = 2 ** (32 - prefix);
  let start = within.addr;
  const end = last(within);
  while (start <= end) {
    const c: Cidr = { addr: start >>> 0, prefix };
    const hit = taken.find((t) => overlaps(t, c));
    if (!hit) return c;
    start = Math.max(last(hit) + 1, start + step);
    start = Math.ceil(start / step) * step;
  }
  return null;
}
