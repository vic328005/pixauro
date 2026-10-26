// Run with Node.js 24+ so the native TypeScript loader reads production modules.
import assert from 'node:assert/strict';
import test from 'node:test';
import { performance } from 'node:perf_hooks';
import { beginStroke, appendStroke, BRUSH_SIZES } from '../src/brush.ts';
import { CanvasRenderer } from '../src/canvasRenderer.ts';
import { viewportGeometryChanged } from '../src/viewportGeometry.ts';

const begin = (
  point = { x: 8, y: 8 },
  size = 1,
  id = '9007199254740993',
  width = 32,
  height = 32,
) => beginStroke('session-A', '42', size, id, point, width, height);
const pixels = (stroke) => {
  const result = [];
  const preview = 'snapshot' in stroke ? stroke.snapshot() : stroke;
  preview.forEachPixel((index) => result.push(index));
  return result;
};

test('single click captures guard and arbitrary precision string color ID', () => {
  const s = begin().snapshot();
  assert.deepEqual(pixels(s), [264]);
  assert.equal(s.sessionId, 'session-A');
  assert.equal(s.revision, '42');
  assert.equal(s.colorId, '9007199254740993');
});
test('all supported square sizes have exact coverage', () => {
  for (const size of BRUSH_SIZES) assert.equal(begin(undefined, size).pixelCount, size * size);
});
test('size two is biased right and down', () => {
  assert.deepEqual(pixels(begin({ x: 2, y: 2 }, 2, '1', 8, 8)), [18, 19, 26, 27]);
});
test('size four retains old editor even-size bias', () => {
  const s = begin({ x: 4, y: 4 }, 4, '1', 10, 10);
  assert(pixels(s).includes(33));
  assert(pixels(s).includes(66));
  assert(!pixels(s).includes(22));
  assert.equal(s.pixelCount, 16);
});
test('large corner stamp clips coordinates without wrapping rows', () => {
  assert.equal(begin({ x: 0, y: 0 }, 8, '1', 8, 8).pixelCount, 25);
  assert.equal(begin({ x: 7, y: 7 }, 8, '1', 8, 8).pixelCount, 16);
});
test('fast horizontal movement has no holes', () => {
  const s = begin({ x: 0, y: 1 }, 1, '1', 8, 8);
  appendStroke(s, { x: 7, y: 1 });
  assert.deepEqual(pixels(s), [8, 9, 10, 11, 12, 13, 14, 15]);
});
test('reverse diagonal interpolation covers both endpoints', () => {
  const s = begin({ x: 4, y: 4 }, 1, '1', 8, 8);
  appendStroke(s, { x: 0, y: 0 });
  assert.deepEqual(pixels(s), [36, 27, 18, 9, 0]);
});
test('steep interpolation maintains 8-connected coverage', () => {
  const s = begin({ x: 1, y: 0 }, 1, '1', 8, 8);
  appendStroke(s, { x: 3, y: 7 });
  assert.equal(s.pixelCount, 8);
  const points = pixels(s).map((i) => [i % 8, Math.floor(i / 8)]);
  for (let i = 1; i < points.length; i++) {
    assert(Math.abs(points[i][0] - points[i - 1][0]) <= 1);
    assert(Math.abs(points[i][1] - points[i - 1][1]) <= 1);
  }
});
test('mutable appends preserve exact immutable preview prefixes', () => {
  const stroke = begin({ x: 1, y: 1 });
  const original = stroke.snapshot();
  const coverage = original.coverage();
  appendStroke(stroke, { x: 5, y: 1 });
  const next = stroke.snapshot();
  assert(Object.isFrozen(original));
  assert.equal(original.pixelCount, 1);
  assert.equal(next.pixelCount, 5);
  assert.deepEqual(pixels(original), [33]);
  assert.deepEqual(original.coverage(), coverage);
  assert.equal(original.key, next.key);
});
test('same point is a no-op and retraced coverage deduplicates', () => {
  const s = begin({ x: 1, y: 1 });
  assert.equal(appendStroke(s, { x: 1, y: 1 }), false);
  assert.equal(appendStroke(s, { x: 5, y: 1 }), true);
  assert.equal(appendStroke(s, { x: 1, y: 1 }), false);
  assert.equal(s.pixelCount, 5);
});
test('leaving canvas inserts a break and does not bridge reentry', () => {
  const s = begin({ x: 1, y: 1 }, 1, '1', 8, 8);
  assert.equal(appendStroke(s, null), false);
  assert.equal(appendStroke(s, null), false);
  appendStroke(s, { x: 6, y: 6 });
  assert.deepEqual(pixels(s), [9, 54]);
});
test('out-of-bounds or noninteger coordinates break the path safely', () => {
  for (const p of [
    { x: -1, y: 0 },
    { x: 8, y: 0 },
    { x: 0, y: 8 },
    { x: 1.5, y: 0 },
    { x: NaN, y: 0 },
  ]) {
    const s = begin({ x: 1, y: 1 }, 1, '1', 8, 8);
    appendStroke(s, p);
    appendStroke(s, { x: 6, y: 6 });
    assert.deepEqual(pixels(s), [9, 54]);
  }
});
test('eraser preserves null target throughout the batch', () => {
  const s = begin({ x: 1, y: 1 }, 4, null);
  appendStroke(s, { x: 4, y: 4 });
  assert.equal(s.colorId, null);
  assert.equal(s.brushSize, 4);
  assert.equal(s.revision, '42');
});
test('unsupported brush sizes reject without creating a preview', () => {
  for (const size of [0, 3, 16, -1]) assert.throws(() => begin(undefined, size), /Unsupported/);
});
test('coverage uses little-endian pixel bits and zeroes trailing padding', () => {
  const s = begin({ x: 0, y: 0 }, 1, '1', 3, 3);
  appendStroke(s, null);
  appendStroke(s, { x: 2, y: 2 });
  assert.deepEqual(s.snapshot().coverage(), [1, 1]);
});
test('more than 65,536 samples retain exact late coverage without path growth', () => {
  const s = begin({ x: 0, y: 0 }, 1, '1', 8, 8);
  const retained = s.retainedBytes;
  for (let i = 0; i < 100_000; i++) appendStroke(s, { x: i % 2, y: 0 });
  appendStroke(s, null);
  appendStroke(s, { x: 7, y: 7 });
  assert.deepEqual(pixels(s), [0, 1, 63]);
  assert.deepEqual(s.snapshot().coverage(), [3, 0, 0, 0, 0, 0, 0, 128]);
  assert.equal(s.retainedBytes, retained);
  assert.equal('points' in s, false);
  assert(s.retainedBytes <= Math.ceil((8 * 8) / 8) + 8 * 8 * 4);
});
test('chunk boundaries preserve stable prefixes and visit only appended pixels', () => {
  const s = begin({ x: 0, y: 0 }, 1, '1', 2048, 1);
  appendStroke(s, { x: 1023, y: 0 });
  const first = s.snapshot();
  appendStroke(s, { x: 2047, y: 0 });
  const extra = [];
  s.snapshot().forEachPixel((index) => extra.push(index), first.pixelCount);
  assert.equal(first.pixelCount, 1024);
  assert.equal(pixels(first).at(-1), 1023);
  assert.equal(extra.length, 1024);
  assert.equal(extra[0], 1024);
  assert.equal(extra.at(-1), 2047);
  assert.equal(s.retainedBytes, 256 + 2048 * 4);
});
test('4,000 brush8 samples stay within an interactive accumulation budget', () => {
  const s = begin({ x: 0, y: 0 }, 8, '1', 1024, 1024);
  const start = performance.now();
  for (let i = 1; i < 4000; i++)
    appendStroke(s, { x: (i * 7) % 1024, y: (Math.floor(i / 147) * 8) % 1024 });
  const elapsed = performance.now() - start;
  assert.equal(s.pixelCount, 219_624);
  assert(elapsed < 2000, `4,000 samples took ${elapsed.toFixed(1)}ms`);
  assert(s.retainedBytes <= Math.ceil((1024 * 1024) / 8) + 1024 * 1024 * 4);
});

const red = { red: 255, green: 0, blue: 0, alpha: 128 };
const blue = { red: 0, green: 0, blue: 255, alpha: 255 };
const makeDocument = () => ({
  sessionId: 'session-A',
  revision: '42',
  width: 8,
  height: 8,
  pixels: new Array(64).fill('blue'),
  palette: [
    { id: 'red', color: red },
    { id: 'blue', color: blue },
  ],
});
const makeContext = () => ({
  allocations: 0,
  uploads: [],
  imageSmoothingEnabled: true,
  createImageData(width, height) {
    this.allocations += 1;
    return { width, height, data: new Uint8ClampedArray(width * height * 4) };
  },
  putImageData(data, ...rectangle) {
    this.uploads.push(rectangle);
    this.data = data;
  },
});
const rgbaAt = (context, index) => Array.from(context.data.data.slice(index * 4, index * 4 + 4));

test('renderer reuses base RGBA and updates only the new preview coverage', () => {
  const renderer = new CanvasRenderer();
  const context = makeContext();
  const document = makeDocument();
  renderer.render(context, document, null, null);
  const s = begin({ x: 1, y: 1 }, 1, 'red', 8, 8);
  renderer.render(context, document, null, s.snapshot());
  assert.deepEqual(context.uploads.at(-1), [0, 0, 1, 1, 1, 1]);
  assert.deepEqual(rgbaAt(context, 9), [255, 0, 0, 128]);
  appendStroke(s, { x: 3, y: 1 });
  renderer.render(context, document, null, s.snapshot());
  assert.equal(context.allocations, 1);
  assert.deepEqual(context.uploads.at(-1), [0, 0, 2, 1, 2, 1]);
  assert.deepEqual(rgbaAt(context, 11), [255, 0, 0, 128]);
  const uploadCount = context.uploads.length;
  renderer.render(context, document, null, s.snapshot());
  assert.equal(context.uploads.length, uploadCount);
  assert.equal(context.imageSmoothingEnabled, false);
});
test('renderer erases with raw transparency and restores canceled previews', () => {
  const renderer = new CanvasRenderer();
  const context = makeContext();
  const document = makeDocument();
  const s = begin({ x: 1, y: 1 }, 1, null, 8, 8);
  renderer.render(context, document, null, s.snapshot());
  assert.deepEqual(rgbaAt(context, 9), [0, 0, 0, 0]);
  renderer.render(context, document, null, null);
  assert.deepEqual(rgbaAt(context, 9), [0, 0, 255, 255]);
  assert.equal(context.allocations, 1);
});
test('renderer refreshes document/palette identities and ignores old sessions', () => {
  const renderer = new CanvasRenderer();
  const context = makeContext();
  const document = makeDocument();
  const s = begin({ x: 1, y: 1 }, 1, 'blue', 8, 8);
  const draft = { id: 'blue', color: red };
  renderer.render(context, document, draft, s.snapshot());
  assert.deepEqual(rgbaAt(context, 9), [255, 0, 0, 128]);
  renderer.render(context, document, null, s.snapshot());
  assert.deepEqual(rgbaAt(context, 9), [0, 0, 255, 255]);
  const changed = { ...document, sessionId: 'session-B', pixels: new Array(64).fill(null) };
  renderer.render(context, changed, null, s.snapshot());
  assert.deepEqual(rgbaAt(context, 9), [0, 0, 0, 0]);
  assert.equal(context.allocations, 3);
});

test('duplicate Dockview dimensions leave the captured stroke open through pointerup', () => {
  const geometry = { left: 200, top: 100, width: 600, height: 500 };
  const s = begin({ x: 0, y: 0 }, 1, '1', 32, 32);
  let active = s;
  const commits = [];
  const measure = (next) => {
    if (viewportGeometryChanged(geometry, next) && active) {
      commits.push(active.snapshot());
      active = null;
    }
  };
  assert.equal(viewportGeometryChanged(null, geometry), false);
  // Starting a stroke changes disableDnd, which triggers an unchanged layout event.
  measure({ ...geometry });
  if (active) appendStroke(active, { x: 12, y: 0 });
  measure({ ...geometry });
  if (active) appendStroke(active, { x: 26, y: 0 });
  assert.equal(commits.length, 0);
  commits.push(active.snapshot());
  assert.equal(commits.length, 1);
  assert.equal(commits[0].pixelCount, 27);
});
test('real viewport moves, resizes, and hidden geometry still interrupt strokes', () => {
  const geometry = { left: 200, top: 100, width: 600, height: 500 };
  for (const change of [
    { left: 201 },
    { top: 101 },
    { width: 601 },
    { height: 501 },
    { width: 0, height: 0 },
  ]) {
    assert.equal(viewportGeometryChanged(geometry, { ...geometry, ...change }), true);
  }
  assert.equal(viewportGeometryChanged(geometry, { ...geometry }), false);
});
