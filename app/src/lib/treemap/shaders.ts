// Sombreadores GLSL ES 3.00 del treemap.

/** Rectángulos instanciados: un quad (TRIANGLE_STRIP de 4) por rectángulo. */
export const BASE_VS = /* glsl */ `#version 300 es
precision highp float;
precision highp int;
layout(location = 0) in vec4 a_rect;   // x0 y0 x1 y1 (px del layout)
layout(location = 1) in vec4 a_grad;   // dz/dx en x0, x1; dz/dy en y0, y1
layout(location = 2) in vec4 a_info;   // depth, cat, flags, parent
layout(location = 3) in vec2 a_mark;   // resaltado anterior / nuevo (0..1)
uniform vec4 u_xform;                  // layout -> px de dispositivo: p·s + t
uniform vec2 u_view;                   // px de dispositivo
uniform float u_snap;                  // 1 = bordes al píxel
out vec2 v_uv;
out vec2 v_pos;
flat out vec4 v_box;
flat out vec4 v_grad;
flat out vec4 v_info;
flat out vec2 v_mark;
void main() {
  bool right = (gl_VertexID & 1) == 1;
  bool bottom = gl_VertexID >= 2;
  vec2 p0 = a_rect.xy * u_xform.xy + u_xform.zw;
  vec2 p1 = a_rect.zw * u_xform.xy + u_xform.zw;
  if (u_snap > 0.5) {
    p0 = floor(p0 + 0.5);
    p1 = floor(p1 + 0.5);
  }
  // selección exacta (no mix): dos vecinos con el mismo borde dan el mismo vértice
  vec2 p = vec2(right ? p1.x : p0.x, bottom ? p1.y : p0.y);
  v_uv = vec2(right ? 1.0 : 0.0, bottom ? 1.0 : 0.0);
  v_pos = p;
  v_box = vec4(p0, p1);
  v_grad = a_grad;
  v_info = a_info;
  v_mark = a_mark;
  vec2 ndc = p / u_view * 2.0 - 1.0;
  gl_Position = vec4(ndc.x, -ndc.y, 0.0, 1.0);
}
`;

export const BASE_FS = /* glsl */ `#version 300 es
precision highp float;
precision highp int;
in vec2 v_uv;
in vec2 v_pos;
flat in vec4 v_box;
flat in vec4 v_grad;
flat in vec4 v_info;
flat in vec2 v_mark;
uniform vec3 u_palette[16];
uniform vec3 u_light;       // normalizado, hacia la luz
uniform float u_ambient;
uniform float u_gain;
uniform float u_spec;
uniform vec3 u_bg;
uniform vec3 u_panel;
uniform vec3 u_header;
uniform vec3 u_border;
uniform float u_headerPx;   // franja de título (px de dispositivo)
uniform float u_dpr;
uniform float u_hiT;        // mezcla del resaltado anterior al nuevo
out vec4 o;

vec3 cushion(vec3 base, vec2 g) {
  vec3 n = normalize(vec3(-g, 1.0));
  float d = max(dot(n, u_light), 0.0);
  vec3 c = base * (u_ambient + (1.0 - u_ambient) * d) * u_gain;
  // brillo especular suave (luz reflejada hacia el observador)
  vec3 r = reflect(-u_light, n);
  c += u_spec * pow(max(r.z, 0.0), 24.0);
  return c;
}

void main() {
  int fl = int(v_info.z + 0.5);
  int cat = int(v_info.y + 0.5);
  float lvl = clamp(v_info.x - 1.0, 0.0, 8.0);
  vec3 col;
  if ((fl & 1) != 0 && (fl & 8) == 0) {
    // marco de carpeta: panel, franja de título y borde de 1 px
    float dy = v_pos.y - v_box.y;
    float de = min(min(v_pos.x - v_box.x, v_box.z - v_pos.x), min(dy, v_box.w - v_pos.y));
    col = u_panel + lvl * 0.010;
    if ((fl & 2) != 0 && dy < u_headerPx) {
      float t = clamp(dy / max(u_headerPx, 1.0), 0.0, 1.0);
      col = u_header + lvl * 0.012 + (1.0 - t) * 0.018;
    }
    if (de < 1.0) col = u_border + lvl * 0.012;
  } else {
    vec3 base = u_palette[cat < 16 ? cat : 0];
    vec2 g = vec2(mix(v_grad.x, v_grad.y, v_uv.x), mix(v_grad.z, v_grad.w, v_uv.y));
    if ((fl & (8 | 16 | 32)) != 0) {
      // resto, carpeta maciza o sintético: apagado y con una trama fina
      float l = dot(base, vec3(0.299, 0.587, 0.114));
      base = (fl & 32) != 0 ? vec3(l * 0.7) : mix(vec3(l), base, 0.6) * 0.86;
      float s = fract((v_pos.x + v_pos.y) / (5.0 * u_dpr));
      base *= 1.0 - 0.10 * step(0.5, s);
    }
    col = cushion(base, g);
  }
  float m = mix(v_mark.x, v_mark.y, u_hiT);
  if (m < 0.999) {
    float l = dot(col, vec3(0.299, 0.587, 0.114));
    vec3 dim = mix(u_bg, vec3(l), 0.28);
    col = mix(dim, col, m);
  }
  o = vec4(clamp(col, 0.0, 1.0), 1.0);
}
`;

/** Triángulo que cubre la pantalla y copia la imagen ya pintada (con fundido opcional). */
export const COMPOSITE_VS = /* glsl */ `#version 300 es
void main() {
  vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
  gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}
`;

export const COMPOSITE_FS = /* glsl */ `#version 300 es
precision highp float;
uniform sampler2D u_cur;
uniform sampler2D u_prev;
uniform float u_fade;   // 1 = solo la actual
out vec4 o;
void main() {
  ivec2 c = ivec2(gl_FragCoord.xy);
  vec4 a = texelFetch(u_cur, c, 0);
  if (u_fade < 1.0) a = mix(texelFetch(u_prev, c, 0), a, u_fade);
  o = a;
}
`;

/** Contornos (cursor, selección): caja con trazo interior, brillo exterior y relleno. */
export const OUTLINE_VS = /* glsl */ `#version 300 es
precision highp float;
layout(location = 0) in vec4 a_box;    // px de dispositivo
layout(location = 1) in vec4 a_color;  // rgba
layout(location = 2) in vec4 a_style;  // trazo px, brillo px, alfa del brillo, alfa del relleno
uniform vec2 u_view;
out vec2 v_pos;
flat out vec4 v_box;
flat out vec4 v_color;
flat out vec4 v_style;
void main() {
  float pad = a_style.y * 2.5 + 2.0;
  vec2 p0 = a_box.xy - pad;
  vec2 p1 = a_box.zw + pad;
  vec2 p = vec2((gl_VertexID & 1) == 1 ? p1.x : p0.x, gl_VertexID >= 2 ? p1.y : p0.y);
  v_pos = p;
  v_box = a_box;
  v_color = a_color;
  v_style = a_style;
  vec2 ndc = p / u_view * 2.0 - 1.0;
  gl_Position = vec4(ndc.x, -ndc.y, 0.0, 1.0);
}
`;

export const OUTLINE_FS = /* glsl */ `#version 300 es
precision highp float;
in vec2 v_pos;
flat in vec4 v_box;
flat in vec4 v_color;
flat in vec4 v_style;
out vec4 o;
float sdBox(vec2 p, vec2 b) {
  vec2 d = abs(p) - b;
  return length(max(d, 0.0)) + min(max(d.x, d.y), 0.0);
}
void main() {
  vec2 c = (v_box.xy + v_box.zw) * 0.5;
  vec2 hs = max((v_box.zw - v_box.xy) * 0.5, vec2(0.5));
  float d = sdBox(v_pos - c, hs);          // < 0 dentro
  float sw = v_style.x;
  float stroke = clamp(sw * 0.5 - abs(d + sw * 0.5) + 0.5, 0.0, 1.0);
  float r = max(v_style.y, 0.001);
  float glow = d > 0.0 ? v_style.z * exp(-(d * d) / (r * r)) : 0.0;
  float fill = d < -sw ? v_style.w : 0.0;
  float a = stroke + (1.0 - stroke) * max(glow, fill);
  a *= v_color.a;
  o = vec4(v_color.rgb * a, a);
}
`;
