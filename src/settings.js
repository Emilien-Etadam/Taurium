let services = [];
let recipes = [];
let editingIndex = -1;
let deleteIndex = -1;
let dragSrcIndex = -1;
let iconDataUrl = ""; // stores base64 data URL for image icon
let savePrefsFeedbackTimer = null;

import { showToast, formatInvokeError, showServicesLoadInfo } from "./toast.js";

function nanoid(size = 10) {
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-';
  let id = '';
  const bytes = crypto.getRandomValues(new Uint8Array(size));
  for (let i = 0; i < size; i++) id += alphabet[bytes[i] & 63];
  return id;
}

function getInvoke() {
  return window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke;
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
    document.getElementById("pref-icon-size").value = prefs.icon_size;
    document.getElementById("pref-icon-size-val").textContent = prefs.icon_size + "px";
    document.getElementById("pref-sidebar-color").value = prefs.sidebar_color;
    document.getElementById("pref-accent-color").value = prefs.accent_color;
    document.getElementById("pref-notifications").checked = prefs.notifications_enabled;
  } catch (err) {
    showToast("Could not load settings: " + formatInvokeError(err), { durationMs: 10000 });
    console.error("Settings init error:", err);
  }

  document.getElementById("add-btn").addEventListener("click", showAddForm);
  document.getElementById("catalog-btn").addEventListener("click", showCatalog);
  document.getElementById("catalog-close").addEventListener("click", hideCatalog);
  document.getElementById("catalog-search").addEventListener("input", renderCatalogList);
  document.getElementById("save-btn").addEventListener("click", saveForm);
  document.getElementById("cancel-btn").addEventListener("click", hideForm);
  document.getElementById("confirm-yes").addEventListener("click", confirmDelete);
  document.getElementById("confirm-no").addEventListener("click", cancelDelete);
  document.getElementById("save-prefs-btn").addEventListener("click", savePreferences);

  // Icon size slider label
  document.getElementById("pref-icon-size").addEventListener("input", (e) => {
    document.getElementById("pref-icon-size-val").textContent = e.target.value + "px";
  });

  // PNG icon file input
  document.getElementById("input-icon-file").addEventListener("change", handleIconFile);
  document.getElementById("icon-preview-clear").addEventListener("click", clearIconPreview);

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

    item.innerHTML = `
      <span class="drag-handle" title="Drag to reorder">&#9776;</span>
      <span class="icon"></span>
      <div class="info">
        <div class="name">${escapeHtml(service.name)}</div>
        <div class="url">${escapeHtml(service.url)}</div>
      </div>
      <div class="actions">
        <button class="btn-icon edit" title="Edit">&#9998;</button>
        <button class="btn-icon delete" title="Delete">&#10005;</button>
      </div>
    `;

    // Render icon safely: image via <img src>, otherwise emoji as text.
    // Never inject the icon through innerHTML (avoids HTML injection).
    const iconSpan = item.querySelector(".icon");
    if (service.icon.startsWith("data:image")) {
      const img = document.createElement("img");
      img.src = service.icon;
      iconSpan.appendChild(img);
    } else {
      iconSpan.textContent = service.icon;
    }
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
  document.getElementById("confirm-msg").textContent = `Delete "${name}"?`;
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

// --- Icon file import ---
function handleIconFile(e) {
  const file = e.target.files[0];
  if (!file) return;

  const reader = new FileReader();
  reader.onload = function(ev) {
    iconDataUrl = ev.target.result;
    document.getElementById("icon-preview-img").src = iconDataUrl;
    document.getElementById("icon-preview").classList.remove("hidden");
    document.getElementById("input-icon").value = "";
    document.getElementById("input-icon").placeholder = "Using image";
  };
  reader.readAsDataURL(file);
}

function clearIconPreview() {
  iconDataUrl = "";
  document.getElementById("icon-preview").classList.add("hidden");
  document.getElementById("input-icon").placeholder = "\uD83D\uDCE7 or use image";
  document.getElementById("input-icon-file").value = "";
}

// --- Form ---
function showAddForm() {
  editingIndex = -1;
  iconDataUrl = "";
  document.getElementById("form-title").textContent = "Add Service";
  document.getElementById("input-name").value = "";
  document.getElementById("input-url").value = "";
  document.getElementById("input-icon").value = "";
  document.getElementById("input-icon").placeholder = "\uD83D\uDCE7 or use image";
  document.getElementById("input-user-agent").value = "";
  document.getElementById("input-zoom").value = "1";
  document.getElementById("input-zoom-val").textContent = "1.0×";
  document.getElementById("icon-preview").classList.add("hidden");
  document.getElementById("input-icon-file").value = "";
  clearErrors();
  document.getElementById("edit-form").classList.remove("hidden");
}

function showEditForm(index) {
  editingIndex = index;
  const s = services[index];
  document.getElementById("form-title").textContent = "Edit Service";
  document.getElementById("input-name").value = s.name;
  document.getElementById("input-url").value = s.url;
  document.getElementById("input-user-agent").value = s.user_agent ?? "";
  const z = s.zoom != null && Number.isFinite(s.zoom) ? s.zoom : 1;
  document.getElementById("input-zoom").value = String(z);
  document.getElementById("input-zoom-val").textContent = Number(z).toFixed(1) + "×";

  // Handle image vs emoji icon
  if (s.icon.startsWith("data:image")) {
    iconDataUrl = s.icon;
    document.getElementById("input-icon").value = "";
    document.getElementById("input-icon").placeholder = "Using image";
    document.getElementById("icon-preview-img").src = s.icon;
    document.getElementById("icon-preview").classList.remove("hidden");
  } else {
    iconDataUrl = "";
    document.getElementById("input-icon").value = s.icon;
    document.getElementById("input-icon").placeholder = "\uD83D\uDCE7 or use image";
    document.getElementById("icon-preview").classList.add("hidden");
  }

  document.getElementById("input-icon-file").value = "";
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
  document.querySelectorAll(".input-error").forEach(el => el.classList.remove("input-error"));
}

function showError(fieldId, message) {
  const input = document.getElementById(fieldId);
  const err = document.getElementById("err-" + fieldId.replace("input-", ""));
  if (input) input.classList.add("input-error");
  if (err) {
    err.textContent = message;
    err.classList.remove("hidden");
  }
}

async function saveForm() {
  clearErrors();
  const name = document.getElementById("input-name").value.trim();
  const url = document.getElementById("input-url").value.trim();
  const userAgentRaw = document.getElementById("input-user-agent").value.trim();
  const user_agent = userAgentRaw.length > 0 ? userAgentRaw : null;
  const zoomRaw = Number.parseFloat(document.getElementById("input-zoom").value);
  const zoomStep = Number.isFinite(zoomRaw) ? Math.round(zoomRaw * 10) / 10 : 1;
  const zoom = zoomStep !== 1 ? zoomStep : null;
  const emojiIcon = document.getElementById("input-icon").value.trim();

  let valid = true;

  // Validate name
  if (!name) {
    showError("input-name", "Name is required");
    valid = false;
  }

  // Validate URL (http/https only)
  if (!url) {
    showError("input-url", "URL is required");
    valid = false;
  } else {
    let parsed = null;
    try {
      parsed = new URL(url);
    } catch {
      parsed = null;
    }
    if (!parsed || (parsed.protocol !== "http:" && parsed.protocol !== "https:")) {
      showError("input-url", "URL must start with http:// or https://");
      valid = false;
    }
  }

  // Validate icon: emoji only (images go through the file picker)
  if (!iconDataUrl && emojiIcon && !isEmojiIcon(emojiIcon)) {
    showError("input-icon", "Icon must be a single emoji (or import an image)");
    valid = false;
  }

  if (!valid) return;

  const id = editingIndex === -1 ? nanoid(10) : services[editingIndex].id;

  // Determine icon: data URL > emoji > default
  let icon;
  if (iconDataUrl) {
    icon = iconDataUrl;
  } else if (emojiIcon) {
    icon = emojiIcon;
  } else {
    icon = "\uD83C\uDF10";
  }

  if (editingIndex === -1) {
    services.push({ id, name, url, icon, user_agent, zoom });
  } else {
    services[editingIndex] = {
      ...services[editingIndex],
      id,
      name,
      url,
      icon,
      user_agent,
      zoom,
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
    showToast("Could not save services: " + formatInvokeError(err));
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
      <span class="icon">${escapeHtml(recipe.icon)}</span>
      <div class="info">
        <div class="name">${escapeHtml(recipe.name)}</div>
        <div class="url">${escapeHtml(recipe.url)}</div>
      </div>
      ${alreadyAdded ? '<span class="catalog-badge">Ajouté</span>' : ""}
    `;
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
  };
  services.push(service);
  hideCatalog();
  renderServices();
  await persistServices();
}

// --- Preferences ---
async function savePreferences() {
  const invoke = getInvoke();
  if (!invoke) return;
  const savePrefsBtn = document.getElementById("save-prefs-btn");

  const prefs = {
    icon_size: parseInt(document.getElementById("pref-icon-size").value),
    sidebar_color: document.getElementById("pref-sidebar-color").value,
    accent_color: document.getElementById("pref-accent-color").value,
    notifications_enabled: document.getElementById("pref-notifications").checked,
  };

  try {
    const savedPrefsJson = await invoke("save_preferences_cmd", { prefs });
    JSON.parse(savedPrefsJson); // Confirms backend returned serialized prefs.

    if (savePrefsFeedbackTimer) clearTimeout(savePrefsFeedbackTimer);
    const originalLabel = "Save Settings";
    savePrefsBtn.textContent = "Saved";
    savePrefsBtn.disabled = true;
    savePrefsFeedbackTimer = setTimeout(() => {
      savePrefsBtn.textContent = originalLabel;
      savePrefsBtn.disabled = false;
      savePrefsFeedbackTimer = null;
    }, 1000);
  } catch (err) {
    showToast("Could not save preferences: " + formatInvokeError(err));
    console.error("Save preferences error:", err);
    savePrefsBtn.textContent = "Save Settings";
    savePrefsBtn.disabled = false;
  }
}

function escapeHtml(text) {
  const div = document.createElement("div");
  div.textContent = text;
  return div.innerHTML;
}

document.addEventListener("DOMContentLoaded", init);
