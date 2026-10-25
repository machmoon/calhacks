import { describe, expect, it } from "vitest";
import { parseGlb } from "./parse";
import { StlError, stlToGlb } from "./stl";

/** A binary STL with the given triangles (9 floats each). */
function stl(tris: number[][]): Uint8Array {
  const out = new Uint8Array(84 + tris.length * 50);
  const dv = new DataView(out.buffer);
  dv.setUint32(80, tris.length, true);
  tris.forEach((t, i) => {
    const at = 84 + i * 50;
    dv.setFloat32(at + 8, 1, true);
    t.forEach((v, k) => dv.setFloat32(at + 12 + k * 4, v, true));
  });
  return out;
}

describe("stlToGlb", () => {
  it("round-trips through the GLB reader with Z-up turned into Y-up", () => {
    const glb = stlToGlb([{ name: "a", bytes: stl([[0, 0, 0, 10, 0, 0, 0, 0, 20]]) }]);
    const scene = parseGlb(glb);
    expect(scene.meshes).toHaveLength(1);
    expect(scene.meshes[0].positions.length).toBe(9);
    // STL z=20 becomes glTF height (y) after the node rotation.
    expect(scene.bounds.max[1]).toBeCloseTo(20, 3);
    expect(scene.bounds.min[2]).toBeCloseTo(0, 3);
  });
  it("makes one mesh per part", () => {
    const t = [[0, 0, 0, 1, 0, 0, 0, 1, 0]];
    expect(parseGlb(stlToGlb([{ name: "a", bytes: stl(t) }, { name: "b", bytes: stl(t) }])).meshes).toHaveLength(2);
  });
  it("refuses a file that is not a binary STL, in words", () => {
    expect(() => stlToGlb([{ name: "x", bytes: new TextEncoder().encode("solid x\nendsolid x\n".padEnd(100)) }])).toThrow(StlError);
    expect(() => stlToGlb([])).toThrow(/No STL/);
  });
});
