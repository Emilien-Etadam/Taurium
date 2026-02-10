let activeId = null;

function getInvoke() {
  return window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke;
}

async function init() {
  // Wait for __TAURI__ to be injected
  let invoke = getInvoke();
  if (!invoke) {
    await new Promise((resolve) => {
      const check = setInterval(() => {
        invoke = getInvoke();
        if (invoke) {
          clearInterval(check);
          resolve();
        }
      }, 50);
    });
  }

  try {
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
  } catch (err) {
    document.body.style.color = "red";
    document.body.innerHTML += "<pre>Init error: " + err + "</pre>";
  }
}

async function switchService(id) {
  const invoke = getInvoke();
  if (!invoke) return;
  try {
    await invoke("switch_service", { id });
    activeId = id;
    updateActiveState();
  } catch (err) {
    document.body.style.color = "red";
    document.body.innerHTML += "<pre>Switch error: " + err + "</pre>";
  }
}

function updateActiveState() {
  document.querySelectorAll(".service-icon").forEach((btn) => {
    btn.classList.toggle("active", btn.dataset.id === activeId);
  });
}

document.addEventListener("DOMContentLoaded", init);
