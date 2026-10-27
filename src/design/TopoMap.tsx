import { useEffect, useRef } from "react";
import { random, tokenColor, useAccent } from "./identity";
import styles from "./TopoMap.module.css";

type TopoMapProps = {
  /** Same seed, same terrain: the map is a person's visual signature. */
  seed: number;
  variant?: "paper" | "night";
  /** Grid step in CSS pixels; smaller draws finer contours. */
  step?: number;
  /** Mark the summits with small survey triangles. */
  summits?: boolean;
  className?: string;
};

type Peak = { x: number; y: number; height: number; spread: number };

function valueNoise(seed: number) {
  const next = random(seed);
  const table = new Float32Array(512);
  for (let index = 0; index < 512; index += 1) table[index] = next();
  const lattice = (x: number, y: number) => table[((x * 73856093) ^ (y * 19349663)) & 511];
  const smooth = (t: number) => t * t * (3 - 2 * t);
  return (x: number, y: number) => {
    const x0 = Math.floor(x), y0 = Math.floor(y);
    const sx = smooth(x - x0), sy = smooth(y - y0);
    const top = lattice(x0, y0) + (lattice(x0 + 1, y0) - lattice(x0, y0)) * sx;
    const bottom = lattice(x0, y0 + 1) + (lattice(x0 + 1, y0 + 1) - lattice(x0, y0 + 1)) * sx;
    return top + (bottom - top) * sy;
  };
}

/** Height field: fractal noise plus a few seeded hills so every map has landmarks. */
function terrain(seed: number, width: number, height: number) {
  const noise = valueNoise(seed);
  const next = random(seed ^ 0x9e3779b9);
  const peaks: Peak[] = Array.from({ length: 3 + Math.floor(next() * 3) }, () => ({
    x: next() * width,
    y: next() * height,
    height: 0.55 + next() * 0.7,
    spread: (0.12 + next() * 0.2) * Math.max(width, height),
  }));
  const scale = 1 / 210;
  const field = (x: number, y: number) => {
    let value = 0, amplitude = 0.55, frequency = scale;
    for (let octave = 0; octave < 4; octave += 1) {
      value += noise(x * frequency + octave * 17.3, y * frequency - octave * 9.1) * amplitude;
      amplitude *= 0.5;
      frequency *= 2.03;
    }
    for (const peak of peaks) {
      const distance = (x - peak.x) ** 2 + (y - peak.y) ** 2;
      value += peak.height * Math.exp(-distance / (2 * peak.spread * peak.spread));
    }
    return value;
  };
  return { field, peaks };
}

// Marching-squares edge pairs for each of the 16 corner cases (0 = top, 1 = right, 2 = bottom, 3 = left).
const SEGMENTS: number[][][] = [
  [], [[3, 2]], [[2, 1]], [[3, 1]], [[0, 1]], [[3, 0], [2, 1]], [[0, 2]], [[3, 0]],
  [[3, 0]], [[0, 2]], [[3, 2], [0, 1]], [[0, 1]], [[3, 1]], [[2, 1]], [[3, 2]], [],
];

function draw(canvas: HTMLCanvasElement, seed: number, variant: "paper" | "night", step: number, summits: boolean) {
  const { width, height } = canvas.getBoundingClientRect();
  if (width < 2 || height < 2) return;
  const ratio = Math.min(window.devicePixelRatio || 1, 2);
  canvas.width = Math.round(width * ratio);
  canvas.height = Math.round(height * ratio);
  const context = canvas.getContext("2d");
  if (!context) return;
  context.setTransform(ratio, 0, 0, ratio, 0, 0);
  context.clearRect(0, 0, width, height);

  const { field, peaks } = terrain(seed, width, height);
  const columns = Math.ceil(width / step) + 1;
  const rows = Math.ceil(height / step) + 1;
  const values = new Float32Array(columns * rows);
  let low = Infinity, high = -Infinity;
  for (let row = 0; row < rows; row += 1) {
    for (let column = 0; column < columns; column += 1) {
      const value = field(column * step, row * step);
      values[row * columns + column] = value;
      low = Math.min(low, value);
      high = Math.max(high, value);
    }
  }

  const levels = 16;
  const line = variant === "night" ? "glow" : "accent";
  context.lineCap = "round";
  for (let level = 1; level < levels; level += 1) {
    const threshold = low + ((high - low) * level) / levels;
    const index = level % 4 === 0;
    context.strokeStyle = tokenColor(line, variant === "night" ? (index ? 0.34 : 0.15) : (index ? 0.3 : 0.14));
    context.lineWidth = index ? 1.25 : 0.7;
    context.beginPath();
    for (let row = 0; row < rows - 1; row += 1) {
      for (let column = 0; column < columns - 1; column += 1) {
        const a = values[row * columns + column];
        const b = values[row * columns + column + 1];
        const c = values[(row + 1) * columns + column + 1];
        const d = values[(row + 1) * columns + column];
        const shape = (a > threshold ? 8 : 0) | (b > threshold ? 4 : 0) | (c > threshold ? 2 : 0) | (d > threshold ? 1 : 0);
        if (shape === 0 || shape === 15) continue;
        const x = column * step, y = row * step;
        const point = (edge: number): [number, number] => {
          const t = (from: number, to: number) => (threshold - from) / (to - from || 1e-6);
          if (edge === 0) return [x + step * t(a, b), y];
          if (edge === 1) return [x + step, y + step * t(b, c)];
          if (edge === 2) return [x + step * t(d, c), y + step];
          return [x, y + step * t(a, d)];
        };
        for (const [from, to] of SEGMENTS[shape]) {
          const [x1, y1] = point(from);
          const [x2, y2] = point(to);
          context.moveTo(x1, y1);
          context.lineTo(x2, y2);
        }
      }
    }
    context.stroke();
  }

  if (summits) {
    context.fillStyle = tokenColor("signal", variant === "night" ? 0.85 : 0.7);
    for (const peak of peaks) {
      if (peak.x < 12 || peak.y < 12 || peak.x > width - 12 || peak.y > height - 12) continue;
      context.beginPath();
      context.moveTo(peak.x, peak.y - 4.5);
      context.lineTo(peak.x + 4, peak.y + 3);
      context.lineTo(peak.x - 4, peak.y + 3);
      context.closePath();
      context.fill();
    }
  }
}

export default function TopoMap({ seed, variant = "paper", step = 9, summits = false, className = "" }: TopoMapProps) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const [accent] = useAccent();
  useEffect(() => {
    const element = canvas.current;
    if (!element) return;
    let frame = 0;
    const render = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => draw(element, seed, variant, step, summits));
    };
    render();
    const observer = new ResizeObserver(render);
    observer.observe(element);
    return () => { observer.disconnect(); cancelAnimationFrame(frame); };
  }, [seed, variant, step, summits, accent]);
  return <canvas ref={canvas} className={`${styles.map} ${className}`} aria-hidden="true" />;
}
