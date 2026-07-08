// V3 Snow — icônes de service en style Lucide (traits fins).
// Une icône de service est soit :
//   - "lucide:<Nom>"  → icône Lucide (stroke 1.7, caps/joins arrondis)
//   - "data:image..." → image importée par l'utilisateur
//   - un emoji        → hérité ; toujours accepté
import { icons } from "lucide";

const SVG_NS = "http://www.w3.org/2000/svg";
const LUCIDE_PREFIX = "lucide:";

export function isLucideIcon(value) {
  return typeof value === "string" && value.startsWith(LUCIDE_PREFIX);
}

export function lucideName(value) {
  return isLucideIcon(value) ? value.slice(LUCIDE_PREFIX.length) : null;
}

export function lucideExists(name) {
  return Object.prototype.hasOwnProperty.call(icons, name);
}

export function allLucideNames() {
  return Object.keys(icons);
}

// Recherche tolérante : « arrow down » trouve AArrowDown, etc.
export function normalizeQuery(s) {
  return s.toLowerCase().replace(/[\s_-]+/g, "");
}

export function lucideEl(name, className) {
  const node = icons[name];
  if (!node) return null;
  const svg = document.createElementNS(SVG_NS, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("fill", "none");
  svg.setAttribute("stroke", "currentColor");
  svg.setAttribute("stroke-width", "1.7");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");
  svg.setAttribute("aria-hidden", "true");
  if (className) svg.setAttribute("class", className);
  for (const [tag, attrs] of node) {
    const el = document.createElementNS(SVG_NS, tag);
    for (const [k, v] of Object.entries(attrs)) {
      el.setAttribute(k, v);
    }
    svg.appendChild(el);
  }
  return svg;
}

// Élément DOM pour une icône de service, quel que soit son type.
// Classes : .icon-lucide (svg), .icon-img (image), .glyph (emoji) — la
// taille est donnée par le contexte CSS.
export function serviceIconEl(icon) {
  if (typeof icon === "string" && icon.startsWith("data:image")) {
    const img = document.createElement("img");
    img.src = icon;
    img.className = "icon-img";
    img.alt = "";
    return img;
  }
  if (isLucideIcon(icon)) {
    const svg = lucideEl(lucideName(icon), "icon-lucide");
    if (svg) return svg;
  }
  const glyph = document.createElement("span");
  glyph.className = "glyph";
  glyph.textContent = icon;
  return glyph;
}
