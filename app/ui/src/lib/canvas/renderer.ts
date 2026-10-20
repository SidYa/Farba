// WebGL2 image renderer.
//
// Every layer has a GpuImage: textures of at most CHUNK x CHUNK pixels (so layers larger
// than MAX_TEXTURE_SIZE work), kept while the document is open so switching tabs is instant.
// Edited 64x64 tiles are re-uploaded with texSubImage2D straight from WASM memory. Textures
// hold premultiplied pixels (WASM premultiplies each tile on the way out), so filtering and
// mipmaps do not darken semi-transparent edges. A frame draws the checkerboard under the
// image, then the visible layers bottom to top with premultiplied "over" blending, clipped
// to the image bounds. The selection mask is a single-channel GpuImage; its outline
// ("marching ants") is found per screen pixel in a shader, so its cost does not depend on
// how complex the selection is.

import { tile_bytes, tile_size, wasmBytes } from "../wasm/core";

const CHUNK = 4096;

const IDENTITY = new Float32Array([1, 0, 0, 0, 1, 0, 0, 0, 1]);

/** Row-major 3x3 (the core's order) -> column-major for uniformMatrix3fv. */
function columnMajor(m: ArrayLike<number>): Float32Array {
  return new Float32Array([m[0]!, m[3]!, m[6]!, m[1]!, m[4]!, m[7]!, m[2]!, m[5]!, m[8]!]);
}

export interface View {
  /** Screen pixels per image pixel. */
  zoom: number;
  /** Screen position (device pixels) of image pixel (0, 0). */
  x: number;
  y: number;
}

/** Where a layer's tiles live: `tilePtr` returns the address of premultiplied tile pixels. */
export interface TileSource {
  tilesX: number;
  tilesY: number;
  tilePtr(tx: number, ty: number): number;
}

export interface DrawLayer {
  image: GpuImage;
  x: number;
  y: number;
  opacity: number;
  /** Transform preview: layer pixels -> image pixels, row-major 3x3 (replaces x, y). */
  h?: ArrayLike<number>;
}

export interface Scene {
  width: number;
  height: number;
  /** Visible layers, bottom to top. */
  layers: DrawLayer[];
  /** Selection mask (a `mask` GpuImage of the image size); its outline is drawn. */
  selection?: GpuImage;
  /** Draw the selection outline moved by this many image pixels (Move tool preview). */
  selectionOffset?: [number, number];
  /** Marching ants animation step. */
  antsPhase?: number;
}

interface Chunk {
  tex: WebGLTexture;
  x: number;
  y: number;
  w: number;
  h: number;
  mipsDirty: boolean;
}

const VS = `#version 300 es
in vec2 a_unit;
uniform vec4 u_rect;      // quad in layer pixels: x, y, w, h
uniform mat3 u_h;         // layer pixels -> image pixels (homogeneous; a translation
                          // for plain layers, any homography for a transform preview)
uniform vec3 u_view;      // zoom, x, y (device pixels)
uniform vec2 u_screen;    // canvas size (device pixels)
out vec2 v_uv;
void main() {
  vec3 img = u_h * vec3(u_rect.xy + a_unit * u_rect.zw, 1.0);
  // Screen position in homogeneous form, so texturing is perspective-correct.
  vec3 scr = vec3(img.xy * u_view.x + u_view.yz * img.z, img.z);
  gl_Position = vec4(scr.x / u_screen.x * 2.0 - scr.z, scr.z - scr.y / u_screen.y * 2.0, 0.0, scr.z);
  v_uv = a_unit;
}`;

const FS = `#version 300 es
precision highp float;
in vec2 v_uv;
uniform sampler2D u_tex;
uniform vec3 u_view;
uniform vec2 u_screen;
uniform bool u_checker;
uniform vec2 u_image;     // image size: layers are clipped to it
uniform float u_opacity;
uniform vec3 u_check_a;
uniform vec3 u_check_b;
uniform float u_check_size;
uniform int u_check_style;  // 0 checkerboard, 1 dots (B on A), 2 solid A
out vec4 o;
void main() {
  if (u_checker) {
    if (u_check_style == 2) {
      o = vec4(u_check_a, 1.0);
    } else if (u_check_style == 1) {
      // Small round dots on a grid, antialiased.
      float pitch = u_check_size * 1.25;
      float d = length(mod(gl_FragCoord.xy, pitch) - pitch * 0.5);
      float r = max(u_check_size * 0.17, 0.7);
      o = vec4(mix(u_check_b, u_check_a, smoothstep(r - 0.8, r + 0.8, d)), 1.0);
    } else {
      vec2 cell = floor(gl_FragCoord.xy / u_check_size);
      o = vec4(mod(cell.x + cell.y, 2.0) < 1.0 ? u_check_a : u_check_b, 1.0);
    }
    return;
  }
  vec2 img = (vec2(gl_FragCoord.x, u_screen.y - gl_FragCoord.y) - u_view.yz) / u_view.x;
  if (img.x < 0.0 || img.y < 0.0 || img.x > u_image.x || img.y > u_image.y) discard;
  o = texture(u_tex, v_uv) * u_opacity;
}`;

// Marching ants: a screen pixel is on the outline when it is selected (coverage >= 50%) and
// one of its four neighbours is not. Drawn per mask chunk; neighbours across a chunk seam
// read the nearest texel of this chunk.
const ANTS_FS = `#version 300 es
precision highp float;
uniform sampler2D u_mask;
uniform vec4 u_rect;      // chunk in image pixels
uniform vec3 u_view;
uniform vec2 u_screen;
uniform vec2 u_image;
uniform float u_phase;
uniform float u_dash;
out vec4 o;
float sel(vec2 scr) {
  vec2 img = floor((scr - u_view.yz) / u_view.x);
  if (img.x < 0.0 || img.y < 0.0 || img.x >= u_image.x || img.y >= u_image.y) return 0.0;
  ivec2 t = ivec2(clamp(img - u_rect.xy, vec2(0.0), u_rect.zw - 1.0));
  return texelFetch(u_mask, t, 0).r >= 0.5 ? 1.0 : 0.0;
}
void main() {
  vec2 scr = vec2(gl_FragCoord.x, u_screen.y - gl_FragCoord.y);
  if (sel(scr) < 0.5) discard;
  float inner = sel(scr + vec2(1.0, 0.0)) * sel(scr - vec2(1.0, 0.0)) *
                sel(scr + vec2(0.0, 1.0)) * sel(scr - vec2(0.0, 1.0));
  if (inner > 0.5) discard;
  float k = mod(floor((scr.x + scr.y) / u_dash + u_phase), 2.0);
  o = vec4(vec3(k), 1.0);
}`;

export class GpuImage {
  private chunks: Chunk[] = [];
  private chunksX: number;

  /** `mask`: one byte per pixel (the selection), no mipmaps. */
  constructor(
    private gl: WebGL2RenderingContext,
    readonly width: number,
    readonly height: number,
    readonly mask = false,
  ) {
    this.chunksX = Math.ceil(width / CHUNK);
    const chunksY = Math.ceil(height / CHUNK);
    for (let cy = 0; cy < chunksY; cy++) {
      for (let cx = 0; cx < this.chunksX; cx++) {
        const x = cx * CHUNK;
        const y = cy * CHUNK;
        const w = Math.min(CHUNK, width - x);
        const h = Math.min(CHUNK, height - y);
        const tex = gl.createTexture()!;
        gl.bindTexture(gl.TEXTURE_2D, tex);
        if (mask) {
          gl.texStorage2D(gl.TEXTURE_2D, 1, gl.R8, w, h);
          gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
          gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
        } else {
          const levels = Math.floor(Math.log2(Math.max(w, h))) + 1;
          gl.texStorage2D(gl.TEXTURE_2D, levels, gl.RGBA8, w, h);
        }
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
        this.chunks.push({ tex, x, y, w, h, mipsDirty: true });
      }
    }
  }

  /** Uploads every tile. */
  uploadAll(src: TileSource) {
    const n = src.tilesX * src.tilesY;
    const all = new Uint32Array(n);
    for (let i = 0; i < n; i++) all[i] = i;
    this.uploadTiles(src, all);
  }

  /** Re-uploads tiles (indices `ty * tilesX + tx`) from WASM memory. */
  uploadTiles(src: TileSource, indices: ArrayLike<number>) {
    const gl = this.gl;
    const ts = tile_size();
    const tb = this.mask ? ts * ts : tile_bytes();
    const format = this.mask ? gl.RED : gl.RGBA;
    const tilesPerChunk = CHUNK / ts;
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
    gl.pixelStorei(gl.UNPACK_ROW_LENGTH, ts);
    let bound: Chunk | undefined;
    for (let k = 0; k < indices.length; k++) {
      const index = indices[k]!;
      const tx = index % src.tilesX;
      const ty = (index - tx) / src.tilesX;
      const chunk =
        this.chunks[
          Math.floor(ty / tilesPerChunk) * this.chunksX + Math.floor(tx / tilesPerChunk)
        ]!;
      if (chunk !== bound) {
        gl.bindTexture(gl.TEXTURE_2D, chunk.tex);
        bound = chunk;
      }
      const px = tx * ts;
      const py = ty * ts;
      const w = Math.min(ts, this.width - px);
      const h = Math.min(ts, this.height - py);
      const ptr = src.tilePtr(tx, ty);
      if (!ptr) continue;
      gl.texSubImage2D(
        gl.TEXTURE_2D,
        0,
        px - chunk.x,
        py - chunk.y,
        w,
        h,
        format,
        gl.UNSIGNED_BYTE,
        wasmBytes(ptr, tb),
      );
      chunk.mipsDirty = true;
    }
    gl.pixelStorei(gl.UNPACK_ROW_LENGTH, 0);
  }

  /** @internal */
  get chunkList(): readonly Chunk[] {
    return this.chunks;
  }

  dispose() {
    for (const c of this.chunks) this.gl.deleteTexture(c.tex);
    this.chunks = [];
  }
}

export interface Theme {
  background: [number, number, number];
  checkA: [number, number, number];
  checkB: [number, number, number];
  /** How transparency shows: checkerboard, dots, or a solid colour (Settings). */
  checkStyle?: 0 | 1 | 2;
  /** The solid colour (style 2). */
  checkSolid?: [number, number, number];
  /** Checker cell size in CSS pixels (dots scale with it). */
  checkSize?: number;
}

export class Renderer {
  readonly gl: WebGL2RenderingContext;
  private prog: WebGLProgram;
  private u: Record<string, WebGLUniformLocation | null> = {};
  private antsProg: WebGLProgram;
  private antsU: Record<string, WebGLUniformLocation | null> = {};
  theme: Theme = {
    background: [0.2, 0.2, 0.2],
    checkA: [0.6, 0.6, 0.6],
    checkB: [0.4, 0.4, 0.4],
  };

  constructor(readonly canvas: HTMLCanvasElement) {
    const gl = canvas.getContext("webgl2", {
      alpha: false,
      antialias: false,
      depth: false,
      stencil: false,
      preserveDrawingBuffer: false,
      // Not `desynchronized`: a low-latency canvas is presented past the compositor and
      // flickers through popups drawn over it (font list, menus) while it redraws.
      powerPreference: "high-performance",
    });
    if (!gl) throw new Error("WebGL2 is not available");
    this.gl = gl;
    this.prog = link(gl, VS, FS);
    this.antsProg = link(gl, VS, ANTS_FS);
    for (const n of [
      "u_mask",
      "u_rect",
      "u_h",
      "u_view",
      "u_screen",
      "u_image",
      "u_phase",
      "u_dash",
    ])
      this.antsU[n] = gl.getUniformLocation(this.antsProg, n);
    for (const n of [
      "u_rect",
      "u_h",
      "u_view",
      "u_screen",
      "u_tex",
      "u_checker",
      "u_image",
      "u_opacity",
      "u_check_a",
      "u_check_b",
      "u_check_size",
      "u_check_style",
    ])
      this.u[n] = gl.getUniformLocation(this.prog, n);

    const vao = gl.createVertexArray();
    gl.bindVertexArray(vao);
    const buf = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, buf);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([0, 0, 1, 0, 0, 1, 1, 1]), gl.STATIC_DRAW);
    const loc = 0; // bound in link()
    gl.enableVertexAttribArray(loc);
    gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
  }

  createImage(width: number, height: number, mask = false): GpuImage {
    return new GpuImage(this.gl, width, height, mask);
  }

  private drawAnts(mask: GpuImage, phase: number, view: View, width: number, height: number) {
    const gl = this.gl;
    const u = this.antsU;
    gl.useProgram(this.antsProg);
    gl.uniformMatrix3fv(u.u_h!, false, IDENTITY);
    gl.disable(gl.BLEND);
    gl.uniform2f(u.u_screen!, width, height);
    gl.uniform3f(u.u_view!, view.zoom, view.x, view.y);
    gl.uniform2f(u.u_image!, mask.width, mask.height);
    gl.uniform1f(u.u_phase!, phase);
    gl.uniform1f(u.u_dash!, 4 * devicePixelRatio);
    gl.uniform1i(u.u_mask!, 0);
    gl.activeTexture(gl.TEXTURE0);
    for (const c of mask.chunkList) {
      gl.bindTexture(gl.TEXTURE_2D, c.tex);
      gl.uniform4f(u.u_rect!, c.x, c.y, c.w, c.h);
      gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
    }
  }

  draw(scene: Scene | undefined, view: View) {
    const gl = this.gl;
    const { width, height } = this.canvas;
    gl.viewport(0, 0, width, height);
    gl.clearColor(...this.theme.background, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
    if (!scene) return;

    gl.useProgram(this.prog);
    gl.uniform2f(this.u.u_screen!, width, height);
    gl.uniform3f(this.u.u_view!, view.zoom, view.x, view.y);
    gl.uniform2f(this.u.u_image!, scene.width, scene.height);

    // Checkerboard under the whole image (shows where it is transparent).
    gl.disable(gl.BLEND);
    gl.uniform1i(this.u.u_checker!, 1);
    const style = this.theme.checkStyle ?? 0;
    gl.uniform1i(this.u.u_check_style!, style);
    gl.uniform3f(
      this.u.u_check_a!,
      ...(style === 2 ? (this.theme.checkSolid ?? this.theme.checkA) : this.theme.checkA),
    );
    gl.uniform3f(this.u.u_check_b!, ...this.theme.checkB);
    gl.uniform1f(this.u.u_check_size!, (this.theme.checkSize ?? 8) * devicePixelRatio);
    gl.uniformMatrix3fv(this.u.u_h!, false, IDENTITY);
    gl.uniform4f(this.u.u_rect!, 0, 0, scene.width, scene.height);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);

    gl.enable(gl.BLEND);
    gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
    gl.uniform1i(this.u.u_checker!, 0);
    gl.uniform1i(this.u.u_tex!, 0);
    gl.activeTexture(gl.TEXTURE0);

    const minify = view.zoom < 1;
    for (const layer of scene.layers) {
      if (layer.opacity <= 0) continue;
      gl.uniform1f(this.u.u_opacity!, layer.opacity);
      // A transform preview is filtered smoothly (it is resampled anyway).
      const smooth = !!layer.h;
      gl.uniformMatrix3fv(
        this.u.u_h!,
        false,
        layer.h ? columnMajor(layer.h) : columnMajor([1, 0, layer.x, 0, 1, layer.y, 0, 0, 1]),
      );
      for (const c of layer.image.chunkList) {
        // Skip chunks outside the viewport (plain layers only).
        if (!layer.h) {
          const sx = (layer.x + c.x) * view.zoom + view.x;
          const sy = (layer.y + c.y) * view.zoom + view.y;
          if (sx > width || sy > height || sx + c.w * view.zoom < 0 || sy + c.h * view.zoom < 0)
            continue;
        }

        gl.bindTexture(gl.TEXTURE_2D, c.tex);
        const mips = minify || smooth;
        if (mips && c.mipsDirty) {
          gl.generateMipmap(gl.TEXTURE_2D);
          c.mipsDirty = false;
        }
        gl.texParameteri(
          gl.TEXTURE_2D,
          gl.TEXTURE_MIN_FILTER,
          mips ? gl.LINEAR_MIPMAP_LINEAR : gl.NEAREST,
        );
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, smooth ? gl.LINEAR : gl.NEAREST);
        gl.uniform4f(this.u.u_rect!, c.x, c.y, c.w, c.h);
        gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
      }
    }

    if (scene.selection) {
      const [dx, dy] = scene.selectionOffset ?? [0, 0];
      const v = { zoom: view.zoom, x: view.x + dx * view.zoom, y: view.y + dy * view.zoom };
      this.drawAnts(scene.selection, scene.antsPhase ?? 0, v, width, height);
    }
  }
}

function link(gl: WebGL2RenderingContext, vs: string, fs: string): WebGLProgram {
  const prog = gl.createProgram()!;
  for (const [type, src] of [
    [gl.VERTEX_SHADER, vs],
    [gl.FRAGMENT_SHADER, fs],
  ] as const) {
    const sh = gl.createShader(type)!;
    gl.shaderSource(sh, src);
    gl.compileShader(sh);
    if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS))
      throw new Error(gl.getShaderInfoLog(sh) ?? "shader");
    gl.attachShader(prog, sh);
  }
  // Both programs share the quad's vertex array.
  gl.bindAttribLocation(prog, 0, "a_unit");
  gl.linkProgram(prog);
  if (!gl.getProgramParameter(prog, gl.LINK_STATUS))
    throw new Error(gl.getProgramInfoLog(prog) ?? "link");
  return prog;
}
