let services = [];
let editingIndex = -1;
let hasChanges = false;

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
}

function renderServices() {
  const list = document.getElementById("service-list");
  list.innerHTML = "";

  services.forEach((service, index) => {
    const item = document.createElement("div");
    item.className = "service-item";
    item.innerHTML = `
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
    item.querySelector(".delete").addEventListener("click", () => deleteService(index));
    list.appendChild(item);
  });
}

function showAddForm() {
  editingIndex = -1;
  document.getElementById("form-title").textContent = "Add Service";
  document.getElementById("input-name").value = "";
  document.getElementById("input-url").value = "";
  document.getElementById("input-icon").value = "";
  document.getElementById("edit-form").classList.remove("hidden");
}

function showEditForm(index) {
  editingIndex = index;
  const s = services[index];
  document.getElementById("form-title").textContent = "Edit Service";
  document.getElementById("input-name").value = s.name;
  document.getElementById("input-url").value = s.url;
  document.getElementById("input-icon").value = s.icon;
  document.getElementById("edit-form").classList.remove("hidden");
}

function hideForm() {
  document.getElementById("edit-form").classList.add("hidden");
}

async function saveForm() {
  const name = document.getElementById("input-name").value.trim();
  const url = document.getElementById("input-url").value.trim();
  const icon = document.getElementById("input-icon").value.trim();

  if (!name || !url) return;

  const id = name.toLowerCase().replace(/[^a-z0-9]/g, "-");

  if (editingIndex === -1) {
    services.push({ id, name, url, icon: icon || "🌐" });
  } else {
    services[editingIndex] = { ...services[editingIndex], name, url, icon: icon || "🌐" };
  }

  hideForm();
  renderServices();
  await persistServices();
}

async function deleteService(index) {
  services.splice(index, 1);
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
