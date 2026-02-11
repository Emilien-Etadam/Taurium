let services = [];
let editingIndex = -1;
let hasChanges = false;
let deleteIndex = -1;
let dragSrcIndex = -1;
let iconDataUrl = ""; // stores base64 data URL for image icon

function getInvoke() {
  return window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke;
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
    renderServices();

    // Load preferences
    const prefs = await invoke("get_preferences");
    document.getElementById("pref-icon-size").value = prefs.icon_size;
    document.getElementById("pref-icon-size-val").textContent = prefs.icon_size + "px";
    document.getElementById("pref-sidebar-color").value = prefs.sidebar_color;
    document.getElementById("pref-accent-color").value = prefs.accent_color;
    document.getElementById("pref-notifications").checked = prefs.notifications_enabled;
  } catch (err) {
    document.body.innerHTML = "<pre style='color:red;padding:20px'>Error: " + err + "</pre>";
  }

  document.getElementById("add-btn").addEventListener("click", showAddForm);
  document.getElementById("save-btn").addEventListener("click", saveForm);
  document.getElementById("cancel-btn").addEventListener("click", hideForm);
  document.getElementById("restart-btn").addEventListener("click", restartApp);
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
}

function renderServices() {
  const list = document.getElementById("service-list");
  list.innerHTML = "";

  services.forEach((service, index) => {
    const item = document.createElement("div");
    item.className = "service-item";
    item.draggable = true;
    item.dataset.index = index;

    // Render icon (emoji or image)
    let iconHtml;
    if (service.icon.startsWith("data:image")) {
      iconHtml = `<img src="${service.icon}" />`;
    } else {
      iconHtml = service.icon;
    }

    item.innerHTML = `
      <span class="drag-handle" title="Drag to reorder">&#9776;</span>
      <span class="icon">${iconHtml}</span>
      <div class="info">
        <div class="name">${escapeHtml(service.name)}</div>
        <div class="url">${escapeHtml(service.url)}</div>
      </div>
      <div class="actions">
        <button class="btn-icon edit" title="Edit">&#9998;</button>
        <button class="btn-icon delete" title="Delete">&#10005;</button>
      </div>
    `;
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
  const emojiIcon = document.getElementById("input-icon").value.trim();

  let valid = true;

  // Validate name
  if (!name) {
    showError("input-name", "Name is required");
    valid = false;
  }

  // Validate URL
  if (!url) {
    showError("input-url", "URL is required");
    valid = false;
  } else {
    try {
      new URL(url);
    } catch {
      showError("input-url", "Invalid URL (must start with https://)");
      valid = false;
    }
  }

  if (!valid) return;

  const id = name.toLowerCase().replace(/[^a-z0-9]/g, "-");

  // Check for duplicate IDs
  const isDuplicate = services.some((s, i) => {
    if (editingIndex >= 0 && i === editingIndex) return false;
    return s.id === id;
  });

  if (isDuplicate) {
    showError("input-name", "A service with this name already exists");
    return;
  }

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
    services.push({ id, name, url, icon });
  } else {
    services[editingIndex] = { ...services[editingIndex], name, url, icon };
  }

  hideForm();
  renderServices();
  await persistServices();
}

async function persistServices() {
  const invoke = getInvoke();
  if (!invoke) return;
  try {
    await invoke("save_services", { services });
    hasChanges = true;
    document.getElementById("restart-btn").classList.remove("hidden");
  } catch (err) {
    alert("Error saving: " + err);
  }
}

// --- Preferences ---
async function savePreferences() {
  const invoke = getInvoke();
  if (!invoke) return;

  const prefs = {
    icon_size: parseInt(document.getElementById("pref-icon-size").value),
    sidebar_color: document.getElementById("pref-sidebar-color").value,
    accent_color: document.getElementById("pref-accent-color").value,
    notifications_enabled: document.getElementById("pref-notifications").checked,
  };

  try {
    await invoke("save_preferences_cmd", { prefs });
    // Notify sidebar to apply new preferences immediately
    // The sidebar is a sibling webview, we can't access it directly
    // But we can restart to apply
    document.getElementById("restart-btn").classList.remove("hidden");
  } catch (err) {
    alert("Error saving preferences: " + err);
  }
}

async function restartApp() {
  const invoke = getInvoke();
  if (!invoke) return;
  try {
    await invoke("restart_app");
  } catch (err) {
    alert("Error restarting: " + err);
  }
}

function escapeHtml(text) {
  const div = document.createElement("div");
  div.textContent = text;
  return div.innerHTML;
}

document.addEventListener("DOMContentLoaded", init);
