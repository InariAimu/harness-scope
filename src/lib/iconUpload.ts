import policy from "../../crates/core/resources/icon-svg-policy.json";

export const MAX_ICON_SIZE = 128;
export const MAX_ICON_BYTES = 1024 * 1024;
const SVG_NS = "http://www.w3.org/2000/svg";
export interface IconCrop { mode: "crop" | "fit"; zoom: number; x: number; y: number }
export const DEFAULT_ICON_CROP: IconCrop = { mode: "crop", zoom: 1, x: 50, y: 50 };
export interface PreparedIcon {
  name: string;
  image: HTMLImageElement;
  width: number;
  height: number;
  svg?: SVGSVGElement;
  dispose: () => void;
}
export interface NormalizedIcon { dataUrl: string; size: number }

export function cropRectangle(width: number, height: number, crop: IconCrop) {
  if (![width, height].every(v => Number.isFinite(v) && v > 0)) throw new Error("Invalid image dimensions");
  const clamp = (v: number, min: number, max: number, fallback: number) => Number.isFinite(v) ? Math.max(min, Math.min(max, v)) : fallback;
  const size = crop.mode === "fit" ? Math.max(width, height) : Math.min(width, height) / clamp(crop.zoom, 1, 4, 1);
  return {
    x: crop.mode === "fit" ? (width - size) / 2 : (width - size) * clamp(crop.x, 0, 100, 50) / 100,
    y: crop.mode === "fit" ? (height - size) / 2 : (height - size) * clamp(crop.y, 0, 100, 50) / 100,
    size,
    output: Math.max(1, Math.min(MAX_ICON_SIZE, Math.round(size))),
  };
}

function safeSvgValue(value: string) {
  if (/[\\@<>\u0000-\u001f\u007f]/.test(value)) return false;
  return !/url\s*\(/i.test(value) || /^url\(#[\w.:-]+\)$/.test(value);
}

function prepareSvg(source: string): SVGSVGElement {
  if (/<!DOCTYPE|<\?/i.test(source.replace(/^\s*<\?xml[^?]*\?>/i, ""))) throw new Error("SVG 不支持文档类型或处理指令 / SVG document types and processing instructions are unsupported");
  const doc = new DOMParser().parseFromString(source, "image/svg+xml");
  if (doc.querySelector("parsererror") || doc.documentElement.localName !== "svg") throw new Error("SVG 格式无效 / Invalid SVG");
  const nodes = Array.from(doc.querySelectorAll("*"));
  if (nodes.length > 4000) throw new Error("SVG 内容过于复杂 / SVG is too complex");
  for (const node of nodes) {
    if (node.namespaceURI !== SVG_NS || !policy.elements.includes(node.localName)) throw new Error(`SVG 含不支持的元素 ${node.localName}；请导出为 PNG / Unsupported SVG element; export as PNG`);
    // Convert supported inline presentation styles to attributes; never mount user markup in the DOM.
    if (node.hasAttribute("style")) {
      const style = (node as SVGElement).style;
      for (const property of Array.from(style)) {
        if (!policy.styles.includes(property)) continue;
        node.setAttribute(property, style.getPropertyValue(property).trim());
      }
      node.removeAttribute("style");
    }
    for (const attr of Array.from(node.attributes)) {
      if (attr.namespaceURI === "http://www.w3.org/2000/xmlns/") continue;
      if (attr.name === "class" || attr.name.startsWith("aria-") || attr.name.startsWith("data-") || attr.name === "role") { node.removeAttributeNode(attr); continue; }
      const value = attr.value.trim().replace(/url\(\s*['"]?#([\w.:-]+)['"]?\s*\)/gi, "url(#$1)");
      if (attr.namespaceURI || !policy.attributes.includes(attr.name) || !safeSvgValue(value)) throw new Error(`SVG 含不支持的属性 ${attr.name} / Unsupported SVG attribute`);
      attr.value = value;
    }
  }
  const svg = doc.documentElement as unknown as SVGSVGElement;
  const box = (svg.getAttribute("viewBox") ?? "").trim().split(/[\s,]+/).map(Number);
  const hasBox = box.length === 4 && box.every(Number.isFinite) && box[2] > 0 && box[3] > 0;
  const dimension = (value: string | null) => value && /^\d*\.?\d+(px)?$/.test(value) ? parseFloat(value) : null;
  const width = dimension(svg.getAttribute("width")) ?? (hasBox ? box[2] : 0);
  const height = dimension(svg.getAttribute("height")) ?? (hasBox ? box[3] : 0);
  if (!(width > 0 && height > 0) || width > 16384 || height > 16384) throw new Error("SVG 需要有效尺寸或 viewBox（最大 16384） / SVG needs valid dimensions or viewBox");
  if (!hasBox) svg.setAttribute("viewBox", `0 0 ${width} ${height}`);
  svg.setAttribute("width", String(width));
  svg.setAttribute("height", String(height));
  return svg;
}

function encodedSvg(svg: SVGSVGElement) {
  const bytes = new TextEncoder().encode(new XMLSerializer().serializeToString(svg));
  if (bytes.length > MAX_ICON_BYTES) throw new Error("SVG 超过 1 MB，请精简或转为 PNG / SVG exceeds 1 MB");
  let binary = "";
  for (let i = 0; i < bytes.length; i += 8192) binary += String.fromCharCode(...bytes.subarray(i, i + 8192));
  return `data:image/svg+xml;base64,${btoa(binary)}`;
}

export async function prepareIcon(file: File): Promise<PreparedIcon> {
  if (!/\.(svg|png|jpe?g|webp)$/i.test(file.name)) throw new Error("支持 SVG、PNG、JPG、WebP / Choose SVG, PNG, JPG or WebP");
  if (file.size > MAX_ICON_BYTES) throw new Error("请选择 1 MB 以内的图标 / Choose an icon up to 1 MB");
  const svg = /\.svg$/i.test(file.name) ? prepareSvg(await file.text()) : undefined;
  const url = svg ? encodedSvg(svg) : URL.createObjectURL(file);
  const dispose = () => { if (!svg) URL.revokeObjectURL(url); };
  const image = new Image();
  image.src = url;
  try {
    await image.decode();
    const width = svg ? Number(svg.getAttribute("width")) : image.naturalWidth;
    const height = svg ? Number(svg.getAttribute("height")) : image.naturalHeight;
    if (!width || !height || width > 16384 || height > 16384 || width * height > 32_000_000) throw new Error("图片尺寸过大，请先缩小 / Image dimensions are too large");
    return { name: Array.from(file.name.replace(/[\u0000-\u001f\u007f]/g, "")).slice(0, 120).join("") || "icon", image, width, height, svg, dispose };
  } catch (error) {
    dispose();
    if (error instanceof Error && !(error instanceof DOMException)) throw error;
    throw new Error("无法读取图片，请检查文件 / Could not decode this image");
  }
}

export function normalizeIcon(source: PreparedIcon, crop: IconCrop): NormalizedIcon {
  const rect = cropRectangle(source.width, source.height, crop);
  if (source.svg) {
    const outer = document.createElementNS(SVG_NS, "svg");
    outer.setAttribute("width", String(rect.output));
    outer.setAttribute("height", String(rect.output));
    outer.setAttribute("viewBox", `${rect.x} ${rect.y} ${rect.size} ${rect.size}`);
    const inner = source.svg.cloneNode(true) as SVGSVGElement;
    inner.setAttribute("x", "0");
    inner.setAttribute("y", "0");
    outer.append(inner);
    return { dataUrl: encodedSvg(outer), size: rect.output };
  }
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = rect.output;
  const context = canvas.getContext("2d");
  if (!context) throw new Error("无法处理图片 / Image processing is unavailable");
  context.imageSmoothingEnabled = true;
  context.imageSmoothingQuality = "high";
  // Destination positioning preserves transparent padding in fit mode.
  const scale = rect.output / rect.size;
  context.drawImage(source.image, -rect.x * scale, -rect.y * scale, source.width * scale, source.height * scale);
  return { dataUrl: canvas.toDataURL("image/png"), size: rect.output };
}
