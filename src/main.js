const { invoke } = window.__TAURI__.core;

let activeId = null;

async function init() {
  const sidebar = document.getElementById("sidebar");
  const services = await invoke("get_services");

  services.forEach((service) => {
    const btn = document.createElement("div");
    btn.className = "service-icon";
    btn.textContent = service.icon;
    btn.title = service.name;
    btn.dataset.id = service.id;
    btn.addEventListener("click", () => switchService(service.id));
    sidebar.appendChild(btn);
  });

  // Restore last active service
  const lastActive = await invoke("get_last_active_service");
  if (lastActive) {
    await switchService(lastActive);
  } else if (services.length > 0) {
    await switchService(services[0].id);
  }
}

async function switchService(id) {
  await invoke("switch_service", { id });
  activeId = id;
  updateActiveState();
}

function updateActiveState() {
  document.querySelectorAll(".service-icon").forEach((btn) => {
    btn.classList.toggle("active", btn.dataset.id === activeId);
  });
}

init();
