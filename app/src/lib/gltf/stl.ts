// Binary STL to a binary glTF, so the one viewer (`ModelViewer`) can show the
// STL files Wright's FreeCAD stage writes. Binary STL layout (80-byte header,
// u32 triangle count, then 50 bytes per triangle: normal, 3 vertices, u16
// attribute) per the format as the stl_io crate and numpy-stl read it. STL
// is Z-up millimetres and glTF is Y-up, so every mesh hangs under a node
// rotated -90 degrees about X; the renderer refits the camera to the bounds.

export class StlError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "StlError";
  }
}

export interface StlPart {
  name: string;
  bytes: Uint8Array;
}

const PALETTE: [number, number, number][] = [
  [0.78, 0.8, 0.84],
  [0.55, 0.7, 0.9],
  [0.9, 0.62, 0.4],
  [0.6, 0.82, 0.6],
  [0.85, 0.55, 0.7],
];

function triangles(part: StlPart): { positions: Float32Array; normals: Float32Array } {
  const { bytes } = part;
  if (bytes.byteLength < 84) throw new StlError(`${part.name}: too short to be a binary STL.`);
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const count = view.getUint32(80, true);
  if (84 + count * 50 !== bytes.byteLength) {
    throw new StlError(`${part.name}: header says ${count} triangles but the size does not match (an ASCII STL is not read).`);
  }
  const positions = new Float32Array(count * 9);
  const normals = new Float32Array(count * 9);
  for (let t = 0; t < count; t += 1) {
    const at = 84 + t * 50;
    const n = [view.getFloat32(at, true), view.getFloat32(at + 4, true), view.getFloat32(at + 8, true)];
    for (let v = 0; v < 3; v += 1) {
      for (let c = 0; c < 3; c += 1) {
        positions[t * 9 + v * 3 + c] = view.getFloat32(at + 12 + v * 12 + c * 4, true);
        normals[t * 9 + v * 3 + c] = n[c];
      }
    }
  }
  return { positions, normals };
}

const pad4 = (n: number) => (4 - (n % 4)) % 4;

/** One GLB with one mesh per STL part. */
export function stlToGlb(parts: StlPart[]): Uint8Array {
  if (parts.length === 0) throw new StlError("No STL files to show.");
  const chunks: Uint8Array[] = [];
  const bufferViews: unknown[] = [];
  const accessors: unknown[] = [];
  const meshes: unknown[] = [];
  const materials: unknown[] = [];
  const nodes: unknown[] = [];
  let offset = 0;
  const push = (data: ArrayBufferView) => {
    const raw = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
    chunks.push(raw);
    const view = { buffer: 0, byteOffset: offset, byteLength: raw.byteLength };
    offset += raw.byteLength;
    return bufferViews.push(view) - 1;
  };

  parts.forEach((part, i) => {
    const { positions, normals } = triangles(part);
    const count = positions.length / 3;
    const min = [Infinity, Infinity, Infinity];
    const max = [-Infinity, -Infinity, -Infinity];
    for (let k = 0; k < positions.length; k += 1) {
      min[k % 3] = Math.min(min[k % 3], positions[k]);
      max[k % 3] = Math.max(max[k % 3], positions[k]);
    }
    const indices = new Uint32Array(count);
    for (let k = 0; k < count; k += 1) indices[k] = k;
    const pos = accessors.push({ bufferView: push(positions), componentType: 5126, count, type: "VEC3", min, max }) - 1;
    const nor = accessors.push({ bufferView: push(normals), componentType: 5126, count, type: "VEC3" }) - 1;
    const idx = accessors.push({ bufferView: push(indices), componentType: 5125, count, type: "SCALAR" }) - 1;
    const [r, g, b] = PALETTE[i % PALETTE.length];
    const material = materials.push({ pbrMetallicRoughness: { baseColorFactor: [r, g, b, 1] } }) - 1;
    const mesh = meshes.push({ name: part.name, primitives: [{ attributes: { POSITION: pos, NORMAL: nor }, indices: idx, material }] }) - 1;
    nodes.push({ name: part.name, mesh, rotation: [-Math.SQRT1_2, 0, 0, Math.SQRT1_2] });
  });

  const json = new TextEncoder().encode(
    JSON.stringify({
      asset: { version: "2.0", generator: "wright stl" },
      scene: 0,
      scenes: [{ nodes: nodes.map((_, i) => i) }],
      nodes,
      meshes,
      materials,
      accessors,
      bufferViews,
      buffers: [{ byteLength: offset }],
    })
  );
  const jsonLen = json.byteLength + pad4(json.byteLength);
  const binLen = offset + pad4(offset);
  const out = new Uint8Array(12 + 8 + jsonLen + 8 + binLen);
  const dv = new DataView(out.buffer);
  dv.setUint32(0, 0x46546c67, true);
  dv.setUint32(4, 2, true);
  dv.setUint32(8, out.byteLength, true);
  dv.setUint32(12, jsonLen, true);
  dv.setUint32(16, 0x4e4f534a, true);
  out.set(json, 20);
  out.fill(0x20, 20 + json.byteLength, 20 + jsonLen);
  const binAt = 20 + jsonLen;
  dv.setUint32(binAt, binLen, true);
  dv.setUint32(binAt + 4, 0x004e4942, true);
  let w = binAt + 8;
  for (const c of chunks) {
    out.set(c, w);
    w += c.byteLength;
  }
  return out;
}
