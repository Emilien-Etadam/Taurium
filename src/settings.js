let services = [];
let recipes = [];
let editingIndex = -1;
let deleteIndex = -1;
let dragSrcIndex = -1;
let iconDataUrl = ""; // stores base64 data URL for image icon
let iconLucide = ""; // nom d’icône Lucide sélectionné (sans préfixe)
let savePrefsFeedbackTimer = null;
let loadedPrefs = {}; // full prefs from backend, so save preserves fields not shown here
let pendingCertTrust = null; // { host, port, fingerprint } awaiting user confirmation

import { showToast, formatInvokeError, showServicesLoadInfo } from "./toast.js";
import { serviceIconEl, lucideEl, allLucideNames, isLucideIcon, lucideName, lucideExists, normalizeQuery } from "./icons.js";
import { checkForUpdate, installAndRelaunch } from "./updater.js";
import { getVersion } from "@tauri-apps/api/app";

let pendingUpdate = null;

function nanoid(size = 10) {
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-';
  let id = '';
  const bytes = crypto.getRandomValues(new Uint8Array(size));
  for (let i = 0; i < size; i++) id += alphabet[bytes[i] & 63];
  return id;
}

// Populate the group autocomplete from groups already used by other services
function refreshGroupSuggestions() {
  const datalist = document.getElementById("group-suggestions");
  if (!datalist) return;
  const groups = [...new Set(services.map((s) => s.group).filter(Boolean))].sort();
  datalist.innerHTML = "";
  groups.forEach((g) => {
    const opt = document.createElement("option");
    opt.value = g;
    datalist.appendChild(opt);
  });
}

function getInvoke() {
  return window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke;
}

// V3 Snow : thème sombre/clair/auto + accent en preset calibré. Le choix
// est appliqué immédiatement en aperçu, persisté via « Enregistrer ».
const ACCENT_PRESETS = ["blue", "emerald", "violet", "gold", "raspberry", "lagoon"];
let selectedTheme = "dark";
let selectedAccent = "blue";

function applySnowPrefs() {
  const root = document.documentElement;
  root.dataset.accent = selectedAccent;
  if (selectedTheme === "light" || selectedTheme === "dark") {
    root.dataset.theme = selectedTheme;
  } else {
    delete root.dataset.theme; // auto : suit prefers-color-scheme
  }
}

function renderThemeControls() {
  document.querySelectorAll("#pref-theme > button").forEach((btn) => {
    btn.classList.toggle("on", btn.dataset.value === selectedTheme);
  });
  document.querySelectorAll("#pref-accent > button").forEach((btn) => {
    btn.classList.toggle("on", btn.dataset.value === selectedAccent);
  });
}

function initThemeControls() {
  document.querySelectorAll("#pref-theme > button, #pref-accent > button").forEach((btn) => {
    btn.addEventListener("click", () => {
      if (btn.parentElement.id === "pref-theme") {
        selectedTheme = btn.dataset.value;
      } else {
        selectedAccent = btn.dataset.value;
      }
      applySnowPrefs();
      renderThemeControls();
    });
  });
}

// An icon is either an emoji or an imported image (data:image...).
// This rejects arbitrary text/HTML typed into the icon field.
function isEmojiIcon(s) {
  if (!s) return false;
  if ([...s].length > 8) return false; // one emoji + modifiers stays short
  if (/[<>A-Za-z0-9]/.test(s)) return false; // no HTML/ASCII text
  return /\p{Extended_Pictographic}/u.test(s);
}

async function init() {
  let invoke = getInvoke();
  if (!invoke) {
    await new Promise((resolve) => {
      const check = setInterval(() => {
        invoke = getInvoke();
        if (invoke) { clearInterval(check); resolve(); }
      }, 50);
    });
  }

  try {
    services = await invoke("get_services");
    recipes = await invoke("get_recipes");
    renderServices();

    const loadInfo = await invoke("get_services_load_info");
    showServicesLoadInfo(loadInfo);

    // Load preferences
    const prefs = await invoke("get_preferences");
    loadedPrefs = prefs;
    selectedTheme = ["auto", "dark", "light"].includes(prefs.theme) ? prefs.theme : "dark";
    selectedAccent = ACCENT_PRESETS.includes(prefs.accent_color) ? prefs.accent_color : "blue";
    applySnowPrefs();
    initThemeControls();
    renderThemeControls();
    document.getElementById("pref-icon-size").value = prefs.icon_size;
    document.getElementById("pref-icon-size-val").textContent = prefs.icon_size + "px";
    document.getElementById("pref-notifications").checked = prefs.notifications_enabled;
    const hibernationSelect = document.getElementById("pref-hibernation");
    hibernationSelect.value = String(prefs.hibernation_minutes ?? 10);
    // A hand-edited preferences.json can hold a value with no matching
    // option; fall back to the default so the select isn't left blank.
    if (hibernationSelect.value !== String(prefs.hibernation_minutes ?? 10)) {
      hibernationSelect.value = "10";
    }
  } catch (err) {
    showToast("Impossible de charger les réglages : " + formatInvokeError(err), { durationMs: 10000 });
    console.error("Settings init error:", err);
  }

  // Updates section
  initUpdates();
  document.getElementById("check-update-btn").addEventListener("click", () => runUpdateCheck(false));
  document.getElementById("install-update-btn").addEventListener("click", installUpdate);

  document.getElementById("add-btn").addEventListener("click", showAddForm);
  document.getElementById("catalog-btn").addEventListener("click", showCatalog);
  document.getElementById("catalog-close").addEventListener("click", hideCatalog);
  document.getElementById("catalog-search").addEventListener("input", renderCatalogList);
  document.getElementById("save-btn").addEventListener("click", saveForm);
  document.getElementById("cancel-btn").addEventListener("click", hideForm);
  document.getElementById("confirm-yes").addEventListener("click", confirmDelete);
  document.getElementById("confirm-no").addEventListener("click", cancelDelete);
  document.getElementById("save-prefs-btn").addEventListener("click", savePreferences);
  document.getElementById("trust-cert-btn").addEventListener("click", handleTrustCertClick);
  document.getElementById("cert-trust-yes").addEventListener("click", confirmTrustCert);
  document.getElementById("cert-trust-no").addEventListener("click", cancelTrustCert);

  // Icon size slider label
  document.getElementById("pref-icon-size").addEventListener("input", (e) => {
    document.getElementById("pref-icon-size-val").textContent = e.target.value + "px";
  });

  // PNG icon file input
  document.getElementById("input-icon-file").addEventListener("change", handleIconFile);
  document.getElementById("icon-preview-clear").addEventListener("click", clearIconPreview);

  // Sélecteur d'icônes Lucide
  document.getElementById("icon-picker-btn").addEventListener("click", showIconPicker);
  document.getElementById("icon-picker-close").addEventListener("click", hideIconPicker);
  document.getElementById("icon-picker-search").addEventListener("input", filterIconPicker);
  document.getElementById("icon-picker-search").addEventListener("keydown", (e) => {
    if (e.key === "Escape") hideIconPicker();
  });
  // Saisir un emoji remplace l'icône Lucide ou l'image sélectionnée
  document.getElementById("input-icon").addEventListener("input", (e) => {
    if (e.target.value.trim()) {
      iconLucide = "";
      iconDataUrl = "";
      document.getElementById("input-icon-file").value = "";
      refreshIconPreview();
    }
  });

  const zoomInput = document.getElementById("input-zoom");
  const zoomVal = document.getElementById("input-zoom-val");
  zoomInput.addEventListener("input", () => {
    zoomVal.textContent = Number(zoomInput.value).toFixed(1) + "×";
  });
}

function renderServices() {
  const list = document.getElementById("service-list");
  list.innerHTML = "";

  services.forEach((service, index) => {
    const item = document.createElement("div");
    item.className = "service-item";
    item.draggable = true;
    item.dataset.index = index;

    // Icônes de chrome : traits fins style Lucide (stroke 1.7, round).
    item.innerHTML = `
      <span class="drag-handle" title="Glisser pour réordonner">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" width="14" height="14" aria-hidden="true">
          <circle cx="9" cy="6" r="1" /><circle cx="15" cy="6" r="1" />
          <circle cx="9" cy="12" r="1" /><circle cx="15" cy="12" r="1" />
          <circle cx="9" cy="18" r="1" /><circle cx="15" cy="18" r="1" />
        </svg>
      </span>
      <span class="icon"></span>
      <div class="info">
        <div class="name">${escapeHtml(service.name)}</div>
        <div class="url">${escapeHtml(service.url)}</div>
      </div>
      <div class="actions">
        <button class="btn-icon edit" title="Modifier" aria-label="Modifier">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" width="14" height="14" aria-hidden="true">
            <path d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z" />
          </svg>
        </button>
        <button class="btn-icon delete" title="Supprimer" aria-label="Supprimer">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" width="14" height="14" aria-hidden="true">
            <path d="M18 6 6 18" /><path d="m6 6 12 12" />
          </svg>
        </button>
      </div>
    `;

    // Rendu sûr de l'icône (jamais via innerHTML) : Lucide, image ou emoji.
    item.querySelector(".icon").appendChild(serviceIconEl(service.icon));
    item.querySelector(".edit").addEventListener("click", (e) => {
      e.stopPropagation();
      showEditForm(index);
    });
    item.querySelector(".delete").addEventListener("click", (e) => {
      e.stopPropagation();
      showDeleteConfirm(index);
    });

    // Drag & drop events
    item.addEventListener("dragstart", onDragStart);
    item.addEventListener("dragover", onDragOver);
    item.addEventListener("dragleave", onDragLeave);
    item.addEventListener("drop", onDrop);
    item.addEventListener("dragend", onDragEnd);

    list.appendChild(item);
  });
}

// --- Drag & Drop (fixed) ---
function onDragStart(e) {
  dragSrcIndex = parseInt(e.currentTarget.dataset.index);
  e.dataTransfer.effectAllowed = "move";
  e.dataTransfer.setData("text/plain", String(dragSrcIndex));
  // Delay adding class so the drag image captures the normal look
  requestAnimationFrame(() => {
    e.currentTarget.classList.add("dragging");
  });
}

function onDragOver(e) {
  e.preventDefault();
  e.dataTransfer.dropEffect = "move";
  const item = e.currentTarget;
  const rect = item.getBoundingClientRect();
  const midY = rect.top + rect.height / 2;
  // Show indicator above or below
  item.classList.remove("drag-over-top", "drag-over-bottom");
  if (e.clientY < midY) {
    item.classList.add("drag-over-top");
  } else {
    item.classList.add("drag-over-bottom");
  }
}

function onDragLeave(e) {
  e.currentTarget.classList.remove("drag-over-top", "drag-over-bottom");
}

function onDrop(e) {
  e.preventDefault();
  const item = e.currentTarget;
  item.classList.remove("drag-over-top", "drag-over-bottom");

  const fromIndex = parseInt(e.dataTransfer.getData("text/plain"));
  let toIndex = parseInt(item.dataset.index);
  if (isNaN(fromIndex) || isNaN(toIndex) || fromIndex === toIndex) return;

  // Determine if we should insert above or below
  const rect = item.getBoundingClientRect();
  const midY = rect.top + rect.height / 2;
  const insertBelow = e.clientY >= midY;

  // Remove the dragged item
  const [moved] = services.splice(fromIndex, 1);

  // Adjust toIndex if needed after removal
  if (fromIndex < toIndex) toIndex--;
  if (insertBelow) toIndex++;

  services.splice(toIndex, 0, moved);
  renderServices();
  persistServices();
}

function onDragEnd(e) {
  // Clean up all drag classes
  document.querySelectorAll(".service-item").forEach(item => {
    item.classList.remove("dragging", "drag-over-top", "drag-over-bottom");
  });
  dragSrcIndex = -1;
}

// --- Delete confirmation ---
function showDeleteConfirm(index) {
  deleteIndex = index;
  const name = services[index].name;
  document.getElementById("confirm-msg").textContent = `Supprimer « ${name} » ?`;
  document.getElementById("confirm-dialog").classList.remove("hidden");
}

function confirmDelete() {
  if (deleteIndex >= 0) {
    services.splice(deleteIndex, 1);
    renderServices();
    persistServices();
  }
  cancelDelete();
}

function cancelDelete() {
  deleteIndex = -1;
  document.getElementById("confirm-dialog").classList.add("hidden");
}

// --- Certificate trust (self-signed / self-hosted services) ---
async function handleTrustCertClick() {
  const url = document.getElementById("input-url").value.trim();
  if (!url) {
    showToast("Indiquez d'abord une URL.");
    return;
  }
  const invoke = getInvoke();
  if (!invoke) return;

  const btn = document.getElementById("trust-cert-btn");
  btn.disabled = true;
  try {
    const info = await invoke("fetch_service_certificate", { url });
    pendingCertTrust = info;
    document.getElementById("cert-trust-host").textContent = `${info.host}:${info.port}`;
    document.getElementById("cert-trust-fingerprint").textContent = info.fingerprint;
    document.getElementById("cert-trust-dialog").classList.remove("hidden");
  } catch (err) {
    showToast("Impossible de récupérer le certificat : " + formatInvokeError(err), { durationMs: 10000 });
    console.error("Fetch certificate error:", err);
  } finally {
    btn.disabled = false;
  }
}

function hideCertTrustDialog() {
  pendingCertTrust = null;
  document.getElementById("cert-trust-dialog").classList.add("hidden");
}

function cancelTrustCert() {
  hideCertTrustDialog();
}

async function confirmTrustCert() {
  const pending = pendingCertTrust;
  hideCertTrustDialog();
  if (!pending) return;

  const invoke = getInvoke();
  if (!invoke) return;
  try {
    await invoke("trust_service_certificate", {
      host: pending.host,
      port: pending.port,
      expectedFingerprint: pending.fingerprint,
    });
    showToast("Certificat approuvé. Réessayez le service.");
  } catch (err) {
    showToast("Échec de la confiance au certificat : " + formatInvokeError(err), { durationMs: 10000 });
    console.error("Trust certificate error:", err);
  }
}

// --- Icône : trois sources (Lucide / emoji / image), un seul aperçu ---
function refreshIconPreview() {
  const slot = document.getElementById("icon-preview-slot");
  const preview = document.getElementById("icon-preview");
  slot.innerHTML = "";
  const value = iconDataUrl || (iconLucide ? "lucide:" + iconLucide : "");
  if (value) {
    slot.appendChild(serviceIconEl(value));
    preview.classList.remove("hidden");
  } else {
    preview.classList.add("hidden");
  }
}

function setLucideIcon(name) {
  iconLucide = name;
  iconDataUrl = "";
  document.getElementById("input-icon").value = "";
  document.getElementById("input-icon-file").value = "";
  refreshIconPreview();
}

function handleIconFile(e) {
  const file = e.target.files[0];
  if (!file) return;

  const reader = new FileReader();
  reader.onload = function(ev) {
    iconDataUrl = ev.target.result;
    iconLucide = "";
    document.getElementById("input-icon").value = "";
    refreshIconPreview();
  };
  reader.readAsDataURL(file);
}

function clearIconPreview() {
  iconDataUrl = "";
  iconLucide = "";
  document.getElementById("input-icon-file").value = "";
  refreshIconPreview();
}

// --- Sélecteur d'icônes Lucide (toutes les icônes, recherche) ---
let iconPickerBuilt = false;

function buildIconPicker() {
  if (iconPickerBuilt) return;
  iconPickerBuilt = true;
  const grid = document.getElementById("icon-picker-grid");
  const frag = document.createDocumentFragment();
  for (const name of allLucideNames()) {
    const cell = document.createElement("button");
    cell.type = "button";
    cell.className = "icon-cell";
    cell.title = name;
    cell.dataset.name = name;
    cell.setAttribute("role", "option");
    cell.appendChild(lucideEl(name));
    cell.addEventListener("click", () => {
      setLucideIcon(name);
      hideIconPicker();
    });
    frag.appendChild(cell);
  }
  grid.appendChild(frag);
}

function filterIconPicker() {
  const q = normalizeQuery(document.getElementById("icon-picker-search").value.trim());
  const grid = document.getElementById("icon-picker-grid");
  let shown = 0;
  grid.querySelectorAll(".icon-cell").forEach((cell) => {
    const match = !q || normalizeQuery(cell.dataset.name).includes(q);
    cell.style.display = match ? "" : "none";
    if (match) shown++;
  });
  document.getElementById("icon-picker-empty").classList.toggle("hidden", shown > 0);
}

function showIconPicker() {
  buildIconPicker();
  const search = document.getElementById("icon-picker-search");
  search.value = "";
  filterIconPicker();
  document.getElementById("icon-picker-dialog").classList.remove("hidden");
  search.focus();
}

function hideIconPicker() {
  document.getElementById("icon-picker-dialog").classList.add("hidden");
}

// --- Form ---
function showAddForm() {
  editingIndex = -1;
  iconDataUrl = "";
  document.getElementById("form-title").textContent = "Ajouter un service";
  document.getElementById("input-name").value = "";
  document.getElementById("input-url").value = "";
  document.getElementById("input-group").value = "";
  refreshGroupSuggestions();
  document.getElementById("input-icon").value = "";
  iconLucide = "";
  document.getElementById("input-user-agent").value = "";
  document.getElementById("input-zoom").value = "1";
  document.getElementById("input-zoom-val").textContent = "1.0×";
  document.getElementById("input-notify").value = "all";
  document.getElementById("input-keep-alive").checked = false;
  document.getElementById("input-icon-file").value = "";
  refreshIconPreview();
  clearErrors();
  document.getElementById("edit-form").classList.remove("hidden");
}

function showEditForm(index) {
  editingIndex = index;
  const s = services[index];
  document.getElementById("form-title").textContent = "Modifier le service";
  document.getElementById("input-name").value = s.name;
  document.getElementById("input-url").value = s.url;
  document.getElementById("input-group").value = s.group ?? "";
  refreshGroupSuggestions();
  document.getElementById("input-user-agent").value = s.user_agent ?? "";
  const z = s.zoom != null && Number.isFinite(s.zoom) ? s.zoom : 1;
  document.getElementById("input-zoom").value = String(z);
  document.getElementById("input-zoom-val").textContent = Number(z).toFixed(1) + "×";
  const notify = s.notify === "badge" || s.notify === "off" ? s.notify : "all";
  document.getElementById("input-notify").value = notify;
  document.getElementById("input-keep-alive").checked = !!s.keep_alive;

  // Icône : image importée, Lucide, ou emoji hérité
  if (s.icon.startsWith("data:image")) {
    iconDataUrl = s.icon;
    iconLucide = "";
    document.getElementById("input-icon").value = "";
  } else if (isLucideIcon(s.icon)) {
    iconDataUrl = "";
    iconLucide = lucideName(s.icon);
    document.getElementById("input-icon").value = "";
  } else {
    iconDataUrl = "";
    iconLucide = "";
    document.getElementById("input-icon").value = s.icon;
  }
  document.getElementById("input-icon-file").value = "";
  refreshIconPreview();
  clearErrors();
  document.getElementById("edit-form").classList.remove("hidden");
}

function hideForm() {
  document.getElementById("edit-form").classList.add("hidden");
  clearErrors();
}

function clearErrors() {
  document.querySelectorAll(".field-error").forEach(el => {
    el.classList.add("hidden");
    el.textContent = "";
  });
  document.querySelectorAll(".is-error").forEach(el => el.classList.remove("is-error"));
}

function showError(fieldId, message) {
  const input = document.getElementById(fieldId);
  const err = document.getElementById("err-" + fieldId.replace("input-", ""));
  if (input) input.classList.add("is-error");
  if (err) {
    err.textContent = message;
    err.classList.remove("hidden");
  }
}

async function saveForm() {
  clearErrors();
  const name = document.getElementById("input-name").value.trim();
  const url = document.getElementById("input-url").value.trim();
  const groupRaw = document.getElementById("input-group").value.trim();
  const group = groupRaw.length > 0 ? groupRaw : null;
  const userAgentRaw = document.getElementById("input-user-agent").value.trim();
  const user_agent = userAgentRaw.length > 0 ? userAgentRaw : null;
  const zoomRaw = Number.parseFloat(document.getElementById("input-zoom").value);
  const zoomStep = Number.isFinite(zoomRaw) ? Math.round(zoomRaw * 10) / 10 : 1;
  const zoom = zoomStep !== 1 ? zoomStep : null;
  // Notification level: "all" is the default and stored as null to keep the file clean.
  const notifyRaw = document.getElementById("input-notify").value;
  const notify = notifyRaw === "badge" || notifyRaw === "off" ? notifyRaw : null;
  const keep_alive = document.getElementById("input-keep-alive").checked;
  const emojiIcon = document.getElementById("input-icon").value.trim();

  let valid = true;

  // Validate name
  if (!name) {
    showError("input-name", "Indiquez un nom.");
    valid = false;
  }

  // Validate URL (http/https only)
  if (!url) {
    showError("input-url", "Indiquez une URL.");
    valid = false;
  } else {
    let parsed = null;
    try {
      parsed = new URL(url);
    } catch {
      parsed = null;
    }
    if (!parsed || (parsed.protocol !== "http:" && parsed.protocol !== "https:")) {
      showError("input-url", "L’URL doit commencer par http:// ou https://.");
      valid = false;
    }
  }

  // Icône saisie librement : un seul emoji (Lucide et image passent par
  // leurs sélecteurs respectifs)
  if (!iconDataUrl && !iconLucide && emojiIcon && !isEmojiIcon(emojiIcon)) {
    showError("input-icon", "Choisissez une icône, un seul emoji, ou importez une image.");
    valid = false;
  }

  if (!valid) return;

  const id = editingIndex === -1 ? nanoid(10) : services[editingIndex].id;

  // Icône retenue : image importée > Lucide > emoji > globe par défaut
  let icon;
  if (iconDataUrl) {
    icon = iconDataUrl;
  } else if (iconLucide && lucideExists(iconLucide)) {
    icon = "lucide:" + iconLucide;
  } else if (emojiIcon) {
    icon = emojiIcon;
  } else {
    icon = "lucide:Globe";
  }

  if (editingIndex === -1) {
    services.push({ id, name, url, icon, user_agent, zoom, group, notify, keep_alive });
  } else {
    services[editingIndex] = {
      ...services[editingIndex],
      id,
      name,
      url,
      icon,
      user_agent,
      zoom,
      group,
      notify,
      keep_alive,
    };
  }

  hideForm();
  renderServices();
  await persistServices();
}

async function persistServices() {
  const invoke = getInvoke();
  if (!invoke) return;
  try {
    await invoke("save_services_cmd", { services });
    const applyResult = await invoke("apply_services");
    if (applyResult && applyResult.filtered_url_count > 0) {
      showServicesLoadInfo(applyResult);
    }
  } catch (err) {
    showToast("Impossible d’enregistrer les services : " + formatInvokeError(err));
    console.error("Save services error:", err);
  }
}

// --- Catalog ---
function serviceUrls() {
  return new Set(services.map((s) => s.url));
}

function showCatalog() {
  document.getElementById("catalog-search").value = "";
  renderCatalogList();
  document.getElementById("catalog-dialog").classList.remove("hidden");
}

function hideCatalog() {
  document.getElementById("catalog-dialog").classList.add("hidden");
}

function renderCatalogList() {
  const list = document.getElementById("catalog-list");
  const query = document.getElementById("catalog-search").value.trim().toLowerCase();
  const existingUrls = serviceUrls();
  list.innerHTML = "";

  const filtered = recipes.filter((recipe) => {
    if (!query) return true;
    return (
      recipe.name.toLowerCase().includes(query) ||
      recipe.url.toLowerCase().includes(query) ||
      recipe.id.toLowerCase().includes(query)
    );
  });

  if (filtered.length === 0) {
    list.innerHTML = '<p class="catalog-empty">Aucun service trouvé.</p>';
    return;
  }

  filtered.forEach((recipe) => {
    const alreadyAdded = existingUrls.has(recipe.url);
    const item = document.createElement("button");
    item.type = "button";
    item.className = "catalog-item" + (alreadyAdded ? " catalog-item-added" : "");
    item.disabled = alreadyAdded;
    item.innerHTML = `
      <span class="icon"></span>
      <div class="info">
        <div class="name">${escapeHtml(recipe.name)}</div>
        <div class="url">${escapeHtml(recipe.url)}</div>
      </div>
      ${alreadyAdded ? '<span class="chip chip--green">Ajouté</span>' : ""}
    `;
    item.querySelector(".icon").appendChild(serviceIconEl(recipe.icon));
    if (!alreadyAdded) {
      item.addEventListener("click", () => addFromCatalog(recipe));
    }
    list.appendChild(item);
  });
}

async function addFromCatalog(recipe) {
  const service = {
    id: nanoid(10),
    name: recipe.name,
    url: recipe.url,
    icon: recipe.icon,
    user_agent: recipe.user_agent ?? null,
    zoom: null,
    notify: null,
    keep_alive: false,
  };
  services.push(service);
  hideCatalog();
  renderServices();
  await persistServices();
}

// --- Preferences ---
async function initUpdates() {
  const versionEl = document.getElementById("current-version");
  try {
    versionEl.textContent = "v" + (await getVersion());
  } catch (err) {
    console.error("getVersion failed:", err);
  }
  // Auto-check silently when the settings page opens
  runUpdateCheck(true);
}

async function runUpdateCheck(silent) {
  const statusEl = document.getElementById("update-status");
  const installBtn = document.getElementById("install-update-btn");
  const checkBtn = document.getElementById("check-update-btn");
  checkBtn.disabled = true;
  if (!silent) statusEl.textContent = "Vérification…";
  try {
    const update = await checkForUpdate();
    if (update) {
      pendingUpdate = update;
      statusEl.textContent = "Mise à jour disponible : v" + update.version;
      installBtn.classList.remove("hidden");
    } else {
      pendingUpdate = null;
      installBtn.classList.add("hidden");
      if (!silent) statusEl.textContent = "Vous êtes à jour.";
    }
  } catch (err) {
    if (!silent) statusEl.textContent = "Échec de la vérification : " + formatInvokeError(err);
    console.error("Update check error:", err);
  } finally {
    checkBtn.disabled = false;
  }
}

async function installUpdate() {
  if (!pendingUpdate) return;
  const statusEl = document.getElementById("update-status");
  const installBtn = document.getElementById("install-update-btn");
  installBtn.disabled = true;
  try {
    statusEl.textContent = "Téléchargement…";
    await installAndRelaunch(pendingUpdate, (event) => {
      if (event.event === "Finished") statusEl.textContent = "Installation…";
    });
    // The app relaunches on success; nothing else to do here.
  } catch (err) {
    statusEl.textContent = "Échec de l'installation : " + formatInvokeError(err);
    installBtn.disabled = false;
    console.error("Update install error:", err);
  }
}

async function savePreferences() {
  const invoke = getInvoke();
  if (!invoke) return;
  const savePrefsBtn = document.getElementById("save-prefs-btn");

  const prefs = {
    // Preserve fields not editable on this page (e.g. sidebar_expanded).
    ...loadedPrefs,
    icon_size: parseInt(document.getElementById("pref-icon-size").value),
    theme: selectedTheme,
    accent_color: selectedAccent,
    notifications_enabled: document.getElementById("pref-notifications").checked,
    hibernation_minutes: parseInt(document.getElementById("pref-hibernation").value, 10),
  };

  try {
    const savedPrefsJson = await invoke("save_preferences_cmd", { prefs });
    JSON.parse(savedPrefsJson); // Confirms backend returned serialized prefs.
    loadedPrefs = prefs;
    applySnowPrefs();

    if (savePrefsFeedbackTimer) clearTimeout(savePrefsFeedbackTimer);
    const originalLabel = "Enregistrer";
    savePrefsBtn.textContent = "Enregistré";
    savePrefsBtn.disabled = true;
    savePrefsFeedbackTimer = setTimeout(() => {
      savePrefsBtn.textContent = originalLabel;
      savePrefsBtn.disabled = false;
      savePrefsFeedbackTimer = null;
    }, 1000);
  } catch (err) {
    showToast("Impossible d’enregistrer les préférences : " + formatInvokeError(err));
    console.error("Save preferences error:", err);
    savePrefsBtn.textContent = "Enregistrer";
    savePrefsBtn.disabled = false;
  }
}

function escapeHtml(text) {
  const div = document.createElement("div");
  div.textContent = text;
  return div.innerHTML;
}

document.addEventListener("DOMContentLoaded", init);
