let services = [];
let editingIndex = -1;
let hasChanges = false;
let deleteIndex = -1;
let dragIndex = -1;

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
  } catch (err) {
    document.body.innerHTML = "<pre style='color:red;padding:20px'>Error: " + err + "</pre>";
  }

  document.getElementById("add-btn").addEventListener("click", showAddForm);
  document.getElementById("save-btn").addEventListener("click", saveForm);
  document.getElementById("cancel-btn").addEventListener("click", hideForm);
  document.getElementById("restart-btn").addEventListener("click", restartApp);
  document.getElementById("confirm-yes").addEventListener("click", confirmDelete);
  document.getElementById("confirm-no").addEventListener("click", cancelDelete);
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
      <span class="icon">${service.icon}</span>
      <div class="info">
        <div class="name">${escapeHtml(service.name)}</div>
        <div class="url">${escapeHtml(service.url)}</div>
      </div>
      <div class="actions">
        <button class="btn-icon edit" title="Edit">&#9998;</button>
        <button class="btn-icon delete" title="Delete">&#10005;</button>
      </div>
    `;
    item.querySelector(".edit").addEventListener("click", () => showEditForm(index));
    item.querySelector(".delete").addEventListener("click", () => showDeleteConfirm(index));

    // Drag & drop events
    item.addEventListener("dragstart", onDragStart);
    item.addEventListener("dragover", onDragOver);
    item.addEventListener("dragleave", onDragLeave);
    item.addEventListener("drop", onDrop);
    item.addEventListener("dragend", onDragEnd);

    list.appendChild(item);
  });
}

// --- Drag & Drop ---
function onDragStart(e) {
  dragIndex = parseInt(e.currentTarget.dataset.index);
  e.currentTarget.classList.add("dragging");
  e.dataTransfer.effectAllowed = "move";
}

function onDragOver(e) {
  e.preventDefault();
  e.dataTransfer.dropEffect = "move";
  e.currentTarget.classList.add("drag-over");
}

function onDragLeave(e) {
  e.currentTarget.classList.remove("drag-over");
}

function onDrop(e) {
  e.preventDefault();
  e.currentTarget.classList.remove("drag-over");
  const dropIndex = parseInt(e.currentTarget.dataset.index);
  if (dragIndex === dropIndex) return;

  // Reorder
  const [moved] = services.splice(dragIndex, 1);
  services.splice(dropIndex, 0, moved);
  renderServices();
  persistServices();
}

function onDragEnd(e) {
  e.currentTarget.classList.remove("dragging");
  dragIndex = -1;
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

// --- Form ---
function showAddForm() {
  editingIndex = -1;
  document.getElementById("form-title").textContent = "Add Service";
  document.getElementById("input-name").value = "";
  document.getElementById("input-url").value = "";
  document.getElementById("input-icon").value = "";
  clearErrors();
  document.getElementById("edit-form").classList.remove("hidden");
}

function showEditForm(index) {
  editingIndex = index;
  const s = services[index];
  document.getElementById("form-title").textContent = "Edit Service";
  document.getElementById("input-name").value = s.name;
  document.getElementById("input-url").value = s.url;
  document.getElementById("input-icon").value = s.icon;
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
  const icon = document.getElementById("input-icon").value.trim();

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

  if (editingIndex === -1) {
    services.push({ id, name, url, icon: icon || "\uD83C\uDF10" });
  } else {
    services[editingIndex] = { ...services[editingIndex], name, url, icon: icon || "\uD83C\uDF10" };
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
