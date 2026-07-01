let container = null;

export function showToast(message, { variant = "error", durationMs = 6000 } = {}) {
  if (!message) return;

  if (!container) {
    container = document.createElement("div");
    container.id = "toast-container";
    container.className = "toast-container";
    document.body.appendChild(container);
  }

  const toast = document.createElement("div");
  toast.className = `toast toast-${variant}`;
  toast.textContent = message;
  container.appendChild(toast);

  requestAnimationFrame(() => toast.classList.add("visible"));

  window.setTimeout(() => {
    toast.classList.remove("visible");
    window.setTimeout(() => toast.remove(), 300);
  }, durationMs);
}

export function formatInvokeError(err) {
  if (err && typeof err === "object" && "message" in err && err.message) {
    return String(err.message);
  }
  return String(err);
}

export function showServicesLoadInfo(info) {
  if (!info) return;

  if (info.load_error) {
    showToast(info.load_error, { variant: "error", durationMs: 10000 });
  }

  if (info.filtered_url_count > 0) {
    const count = info.filtered_url_count;
    const label = count === 1 ? "service" : "services";
    showToast(
      `${count} ${label} ignored: invalid or non-http(s) URL.`,
      { variant: "warning", durationMs: 8000 },
    );
  }
}
