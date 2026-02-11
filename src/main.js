let activeId = null;
let settingsOpen = false;
let services = [];

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
    // Load preferences and apply theme
    const prefs = await invoke("get_preferences");
    applyPreferences(prefs);

    const serviceList = document.getElementById("service-list");
    services = await invoke("get_services");

    // Show empty state if no services
    if (services.length === 0) {
      document.getElementById("empty-state").classList.remove("hidden");
    }

    services.forEach((service, index) => {
      const btn = document.createElement("div");
      btn.className = "service-icon";
      btn.dataset.id = service.id;
      btn.title = service.name + (index < 9 ? " (Ctrl+" + (index + 1) + ")" : "");

      // Support both emoji and image icons
      if (service.icon.startsWith("data:image")) {
        const img = document.createElement("img");
        img.src = service.icon;
        img.className = "icon-img";
        btn.appendChild(img);
      } else {
        btn.textContent = service.icon;
      }

      btn.addEventListener("click", () => switchService(service.id));
      // Right-click shows native context menu via Tauri
      btn.addEventListener("contextmenu", (e) => {
        e.preventDefault();
        invoke("show_service_context_menu", { id: service.id });
      });
      serviceList.appendChild(btn);
    });

    // Settings button
    document.getElementById("settings-btn").addEventListener("click", openSettings);

    // Keyboard shortcuts
    document.addEventListener("keydown", handleKeyboard);

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

function applyPreferences(prefs) {
  const root = document.documentElement;
  root.style.setProperty("--icon-size", prefs.icon_size + "px");
  root.style.setProperty("--sidebar-color", prefs.sidebar_color);
  root.style.setProperty("--accent-color", prefs.accent_color);
}

// Called from settings webview after prefs change (via Rust eval)
window.__applyPreferences = function(prefs) {
  applyPreferences(prefs);
};

async function switchService(id) {
  const invoke = getInvoke();
  if (!invoke) return;

  // Show loading spinner
  const overlay = document.getElementById("loading-overlay");
  overlay.classList.remove("hidden");

  try {
    await invoke("switch_service", { id });
    activeId = id;
    settingsOpen = false;
    updateActiveState();
  } catch (err) {
    console.error("Switch error:", err);
  }

  // Hide loading after a short delay (webview takes a moment to show)
  setTimeout(() => overlay.classList.add("hidden"), 500);
}

async function openSettings() {
  const invoke = getInvoke();
  if (!invoke) return;
  try {
    await invoke("open_settings");
    activeId = null;
    settingsOpen = true;
    updateActiveState();
  } catch (err) {
    console.error("Settings error:", err);
  }
}

function updateActiveState() {
  document.querySelectorAll(".service-icon").forEach((btn) => {
    btn.classList.toggle("active", btn.dataset.id === activeId);
  });
  document.getElementById("settings-btn").classList.toggle("active", settingsOpen);
}

// Keyboard shortcuts (Ctrl+1-9, Ctrl+,)
function handleKeyboard(e) {
  if (!e.ctrlKey && !e.metaKey) return;

  // Ctrl+, = settings
  if (e.key === ",") {
    e.preventDefault();
    openSettings();
    return;
  }

  // Ctrl+1-9 = switch service
  const num = parseInt(e.key);
  if (num >= 1 && num <= 9 && num <= services.length) {
    e.preventDefault();
    switchService(services[num - 1].id);
  }
}

// Badge update callback (called from Rust via eval)
window.__updateBadges = function(badges) {
  document.querySelectorAll(".service-icon").forEach((btn) => {
    const id = btn.dataset.id;
    // Remove existing badge
    const existing = btn.querySelector(".badge");
    if (existing) existing.remove();

    const count = badges[id];
    if (count && count > 0) {
      const badge = document.createElement("span");
      badge.className = "badge";
      badge.textContent = count > 99 ? "99+" : count;
      btn.appendChild(badge);
    }
  });
};

document.addEventListener("DOMContentLoaded", init);
