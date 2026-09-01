const { invoke } = window.__TAURI__.core;

// ---------- Abas ----------

document.querySelectorAll(".tab-btn").forEach((btn) => {
  btn.addEventListener("click", () => {
    const previous = document.querySelector(".tab-btn.active")?.dataset.tab;
    document.querySelectorAll(".tab-btn").forEach((b) => b.classList.remove("active"));
    document.querySelectorAll(".tab-panel").forEach((p) => p.classList.remove("active"));
    btn.classList.add("active");
    document.querySelector(`#tab-${btn.dataset.tab}`).classList.add("active");

    if (previous === "desempenho" && btn.dataset.tab !== "desempenho") {
      invoke("stop_perf_monitor").catch(() => {});
    }
    if (btn.dataset.tab === "desempenho") {
      invoke("start_perf_monitor").catch((e) => console.error(e));
    }
  });
});

const listEl = document.querySelector("#category-list");
const statusEl = document.querySelector("#status");
const selectAllEl = document.querySelector("#select-all");

let categories = [];

function setStatus(text, kind = "") {
  statusEl.textContent = text;
  statusEl.className = "status" + (kind ? ` ${kind}` : "");
}

function renderCategories() {
  listEl.innerHTML = "";
  for (const cat of categories) {
    const li = document.createElement("li");
    li.className = "category-item";

    const label = document.createElement("label");

    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.dataset.id = cat.id;
    checkbox.checked = false;
    checkbox.disabled = !cat.exists || cat.size_bytes === 0;

    const name = document.createElement("span");
    name.className = "category-name";
    name.textContent = cat.label;
    if (cat.id === "recycle_bin") {
      const warn = document.createElement("span");
      warn.className = "warn-badge";
      warn.textContent = "irreversível";
      name.appendChild(document.createTextNode(" "));
      name.appendChild(warn);
    }

    const size = document.createElement("span");
    size.className = "category-size";
    size.textContent = cat.exists ? cat.size_human : "vazio";

    label.appendChild(checkbox);
    label.appendChild(name);
    li.appendChild(label);
    li.appendChild(size);
    listEl.appendChild(li);
  }
  selectAllEl.checked = false;
}

async function scan() {
  setStatus("Verificando...");
  listEl.innerHTML = "";
  try {
    categories = await invoke("scan_categories");
    renderCategories();
    setStatus("");
  } catch (e) {
    setStatus(`Erro ao verificar: ${e}`, "error");
  }
}

function getSelectedIds() {
  return Array.from(listEl.querySelectorAll("input[type=checkbox]:checked")).map(
    (el) => el.dataset.id
  );
}

async function clean() {
  let ids = getSelectedIds();
  if (ids.length === 0) {
    setStatus("Nenhuma categoria selecionada.");
    return;
  }

  if (ids.includes("recycle_bin")) {
    const ok = window.confirm(
      "Esvaziar a lixeira apaga os itens PERMANENTEMENTE. Continuar mesmo assim?"
    );
    if (!ok) {
      ids = ids.filter((id) => id !== "recycle_bin");
      if (ids.length === 0) {
        setStatus("Cancelado.");
        return;
      }
    }
  }

  setStatus("Limpando...");
  try {
    const results = await invoke("clean_categories", { ids });
    const totalFreed = results.reduce((sum, r) => sum + r.freed_bytes, 0);
    const errors = results.filter((r) => r.error);
    const totalHuman = formatBytes(totalFreed);

    if (errors.length > 0) {
      setStatus(
        `Espaço liberado: ${totalHuman}. Alguns itens não puderam ser removidos (em uso ou sem permissão).`,
        "warn"
      );
    } else {
      setStatus(`Espaço liberado: ${totalHuman}.`, "success");
    }
    await scan();
  } catch (e) {
    setStatus(`Erro ao limpar: ${e}`, "error");
  }
}

// ---------- Limpeza avançada (Espaço Profundo) ----------

const deepSpaceStatusEl = document.querySelector("#deepspace-status");
const windowsOldSizeEl = document.querySelector("#windowsold-size");
const hiberSizeEl = document.querySelector("#hiber-size");
const toggleHibernation = document.querySelector("#toggle-hibernation");
const winsxsOutputEl = document.querySelector("#winsxs-output");
const btnRemoveWindowsOld = document.querySelector("#btn-remove-windowsold");

function setDeepSpaceStatus(text, kind = "") {
  deepSpaceStatusEl.textContent = text;
  deepSpaceStatusEl.className = "status" + (kind ? ` ${kind}` : "");
}

async function loadDeepSpaceInfo() {
  try {
    const info = await invoke("get_deep_space_info");
    windowsOldSizeEl.textContent = info.windows_old_exists
      ? `(${info.windows_old_size_human})`
      : "(não encontrado)";
    btnRemoveWindowsOld.disabled = !info.windows_old_exists;
    hiberSizeEl.textContent = info.hibernation_enabled
      ? `(ativa, ${info.hiberfil_size_human})`
      : "(desativada)";
    toggleHibernation.checked = info.hibernation_enabled;
  } catch (e) {
    setDeepSpaceStatus(`Erro: ${e}`, "error");
  }
}

btnRemoveWindowsOld.addEventListener("click", async () => {
  btnRemoveWindowsOld.disabled = true;
  setDeepSpaceStatus("Removendo Windows.old... uma janela de permissão pode aparecer.");
  try {
    const msg = await invoke("remove_windows_old");
    setDeepSpaceStatus(msg, "success");
    await loadDeepSpaceInfo();
  } catch (e) {
    setDeepSpaceStatus(`Erro: ${e}`, "error");
  } finally {
    btnRemoveWindowsOld.disabled = false;
  }
});

toggleHibernation.addEventListener("change", async () => {
  const enabled = toggleHibernation.checked;
  setDeepSpaceStatus("Aplicando...");
  try {
    const msg = await invoke("set_hibernation", { enabled });
    setDeepSpaceStatus(msg, "success");
    await loadDeepSpaceInfo();
  } catch (e) {
    toggleHibernation.checked = !enabled;
    setDeepSpaceStatus(`Erro: ${e}`, "error");
  }
});

document.querySelector("#btn-analyze-winsxs").addEventListener("click", async () => {
  setDeepSpaceStatus("Analisando componentes do Windows... isso pode levar um tempo.");
  winsxsOutputEl.textContent = "";
  try {
    const out = await invoke("analyze_component_store");
    winsxsOutputEl.textContent = out || "(sem saída)";
    setDeepSpaceStatus("Análise concluída.", "success");
  } catch (e) {
    setDeepSpaceStatus(`Erro: ${e}`, "error");
  }
});

document.querySelector("#btn-clean-winsxs").addEventListener("click", async () => {
  setDeepSpaceStatus("Limpando componentes do Windows... isso pode levar vários minutos.");
  winsxsOutputEl.textContent = "";
  try {
    const out = await invoke("clean_component_store");
    winsxsOutputEl.textContent = out || "(sem saída)";
    setDeepSpaceStatus("Limpeza de componentes concluída.", "success");
  } catch (e) {
    setDeepSpaceStatus(`Erro: ${e}`, "error");
  }
});

loadDeepSpaceInfo();

// ---------- Inicialização ----------

const startupListEl = document.querySelector("#startup-list");
const startupStatusEl = document.querySelector("#startup-status");

function setStartupStatus(text, kind = "") {
  startupStatusEl.textContent = text;
  startupStatusEl.className = "status" + (kind ? ` ${kind}` : "");
}

async function loadStartupItems() {
  setStartupStatus("Verificando...");
  startupListEl.innerHTML = "";
  try {
    const items = await invoke("list_startup_items");
    for (const item of items) {
      const li = document.createElement("li");
      li.className = "category-item";

      const left = document.createElement("div");
      const name = document.createElement("div");
      name.className = "category-name";
      name.textContent = item.name;
      const source = document.createElement("div");
      source.className = "category-size";
      source.textContent = item.source;
      left.appendChild(name);
      left.appendChild(source);

      const label = document.createElement("label");
      label.className = "switch";
      const checkbox = document.createElement("input");
      checkbox.type = "checkbox";
      checkbox.checked = item.enabled;
      checkbox.addEventListener("change", async () => {
        const enabled = checkbox.checked;
        try {
          await invoke("set_startup_item_enabled", { id: item.id, enabled });
          setStartupStatus(`"${item.name}" ${enabled ? "ativado" : "desativado"}.`, "success");
        } catch (e) {
          checkbox.checked = !enabled;
          setStartupStatus(`Erro: ${e}`, "error");
        }
      });
      const slider = document.createElement("span");
      slider.className = "slider";
      label.appendChild(checkbox);
      label.appendChild(slider);

      li.appendChild(left);
      li.appendChild(label);
      startupListEl.appendChild(li);
    }
    setStartupStatus(`${items.length} itens de inicialização encontrados.`);
  } catch (e) {
    setStartupStatus(`Erro: ${e}`, "error");
  }
}

document.querySelector("#btn-scan-startup").addEventListener("click", loadStartupItems);
loadStartupItems();

// ---------- Confiabilidade ----------

const reliabilityListEl = document.querySelector("#reliability-list");
const reliabilityStatusEl = document.querySelector("#reliability-status");
const bootInfoLineEl = document.querySelector("#boot-info-line");

function setReliabilityStatus(text, kind = "") {
  reliabilityStatusEl.textContent = text;
  reliabilityStatusEl.className = "status" + (kind ? ` ${kind}` : "");
}

async function loadBootInfo() {
  try {
    const info = await invoke("get_boot_info");
    bootInfoLineEl.textContent = `Ligado há ${info.uptime_days} dia(s) e ${info.uptime_hours} hora(s) — última inicialização em ${info.last_boot}.`;
  } catch (e) {
    bootInfoLineEl.textContent = `Erro ao obter tempo ligado: ${e}`;
  }
}

async function loadReliabilityEvents() {
  setReliabilityStatus("Verificando eventos dos últimos 30 dias...");
  reliabilityListEl.innerHTML = "";
  try {
    const events = await invoke("get_reliability_events");
    if (events.length === 0) {
      setReliabilityStatus("Nenhum evento de falha nos últimos 30 dias.", "success");
    } else {
      for (const ev of events) {
        const li = document.createElement("li");
        li.className = "category-item";
        const left = document.createElement("div");
        const kind = document.createElement("div");
        kind.className = "category-name";
        kind.textContent = ev.kind;
        const desc = document.createElement("div");
        desc.className = "category-size";
        desc.textContent = `${ev.time}${ev.description ? " — " + ev.description : ""}`;
        left.appendChild(kind);
        left.appendChild(desc);
        li.appendChild(left);
        reliabilityListEl.appendChild(li);
      }
      setReliabilityStatus(`${events.length} evento(s) encontrado(s).`, "warn");
    }
  } catch (e) {
    setReliabilityStatus(`Erro: ${e}`, "error");
  }
}

document.querySelector("#btn-scan-reliability").addEventListener("click", () => {
  loadBootInfo();
  loadReliabilityEvents();
});
loadBootInfo();
loadReliabilityEvents();

// ---------- Desempenho ----------

const perfCpuEl = document.querySelector("#perf-cpu");
const perfMemEl = document.querySelector("#perf-mem");
const perfDiskEl = document.querySelector("#perf-disk");
const perfMemDetailEl = document.querySelector("#perf-mem-detail");
const perfProcessListEl = document.querySelector("#perf-process-list");

window.__TAURI__.event.listen("perf-tick", (event) => {
  const d = event.payload;
  perfCpuEl.textContent = `${Math.round(d.CpuPct ?? 0)}%`;
  perfMemEl.textContent = `${Math.round(d.MemUsedPct ?? 0)}%`;
  perfDiskEl.textContent = `${Math.round(d.DiskPct ?? 0)}%`;
  perfMemDetailEl.textContent = `${(d.MemUsedMB ?? 0).toLocaleString("pt-BR")} MB de ${(d.MemTotalMB ?? 0).toLocaleString("pt-BR")} MB em uso`;

  perfProcessListEl.innerHTML = "";
  for (const p of d.Top ?? []) {
    const li = document.createElement("li");
    li.className = "category-item";
    const left = document.createElement("div");
    const name = document.createElement("div");
    name.className = "category-name";
    name.textContent = p.Name;
    const detail = document.createElement("div");
    detail.className = "category-size";
    detail.textContent = `PID ${p.Pid} — CPU total: ${p.CpuS ?? 0}s`;
    left.appendChild(name);
    left.appendChild(detail);

    const mem = document.createElement("span");
    mem.className = "category-size";
    mem.textContent = `${p.MemMB} MB`;

    li.appendChild(left);
    li.appendChild(mem);
    perfProcessListEl.appendChild(li);
  }
});

function formatBytes(bytes) {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let size = bytes;
  let i = 0;
  while (size >= 1024 && i < units.length - 1) {
    size /= 1024;
    i++;
  }
  return i === 0 ? `${size} ${units[i]}` : `${size.toFixed(2)} ${units[i]}`;
}

async function flushDns() {
  setStatus("Limpando cache de DNS...");
  try {
    const msg = await invoke("flush_dns");
    setStatus(msg, "success");
  } catch (e) {
    setStatus(`Erro: ${e}`, "error");
  }
}

selectAllEl.addEventListener("change", () => {
  const checked = selectAllEl.checked;
  listEl.querySelectorAll("input[type=checkbox]").forEach((el) => {
    if (!el.disabled) el.checked = checked;
  });
});

document.querySelector("#btn-scan").addEventListener("click", scan);
document.querySelector("#btn-clean").addEventListener("click", clean);
document.querySelector("#btn-flush-dns").addEventListener("click", flushDns);

// ---------- Otimizações para gamers ----------

const powerPlanListEl = document.querySelector("#power-plan-list");
const gamerStatusEl = document.querySelector("#gamer-status");
const toggleGameMode = document.querySelector("#toggle-game-mode");
const toggleGameDvr = document.querySelector("#toggle-game-dvr");
const toggleHags = document.querySelector("#toggle-hags");

function setGamerStatus(text, kind = "") {
  gamerStatusEl.textContent = text;
  gamerStatusEl.className = "status" + (kind ? ` ${kind}` : "");
}

async function loadPowerPlans() {
  try {
    const plans = await invoke("list_power_plans");
    powerPlanListEl.innerHTML = "";
    for (const plan of plans) {
      const li = document.createElement("li");
      li.className = "plan-item";
      const label = document.createElement("label");
      const radio = document.createElement("input");
      radio.type = "radio";
      radio.name = "power-plan";
      radio.value = plan.guid;
      radio.checked = plan.active;
      radio.addEventListener("change", async () => {
        setGamerStatus("Ativando plano de energia...");
        try {
          await invoke("set_power_plan", { guid: plan.guid });
          setGamerStatus(`Plano "${plan.name}" ativado.`, "success");
          await loadPowerPlans();
        } catch (e) {
          setGamerStatus(`Erro: ${e}`, "error");
        }
      });
      label.appendChild(radio);
      label.appendChild(document.createTextNode(" " + plan.name));
      li.appendChild(label);
      powerPlanListEl.appendChild(li);
    }
  } catch (e) {
    setGamerStatus(`Erro ao listar planos de energia: ${e}`, "error");
  }
}

async function loadGamerStatus() {
  try {
    const status = await invoke("get_gamer_status");
    toggleGameMode.checked = status.game_mode;
    toggleGameDvr.checked = status.game_dvr;
    if (status.hags === null) {
      toggleHags.checked = false;
      toggleHags.disabled = true;
    } else {
      toggleHags.checked = status.hags;
      toggleHags.disabled = false;
    }
    if (!status.is_admin) {
      toggleHags.title = "Feche e abra o app como Administrador para alterar essa opção";
    }
  } catch (e) {
    setGamerStatus(`Erro ao ler status: ${e}`, "error");
  }
}

document.querySelector("#btn-ultimate").addEventListener("click", async () => {
  setGamerStatus("Habilitando Desempenho Máximo...");
  try {
    await invoke("enable_ultimate_performance");
    setGamerStatus('Plano "Desempenho Máximo" habilitado e ativado.', "success");
    await loadPowerPlans();
  } catch (e) {
    setGamerStatus(`Erro: ${e}`, "error");
  }
});

toggleGameMode.addEventListener("change", async () => {
  const enabled = toggleGameMode.checked;
  try {
    await invoke("set_game_mode", { enabled });
    setGamerStatus(`Modo de Jogo ${enabled ? "ativado" : "desativado"}.`, "success");
  } catch (e) {
    toggleGameMode.checked = !enabled;
    setGamerStatus(`Erro: ${e}`, "error");
  }
});

toggleGameDvr.addEventListener("change", async () => {
  const enabled = toggleGameDvr.checked;
  try {
    await invoke("set_game_dvr", { enabled });
    setGamerStatus(`Gravação em segundo plano ${enabled ? "ativada" : "desativada"}.`, "success");
  } catch (e) {
    toggleGameDvr.checked = !enabled;
    setGamerStatus(`Erro: ${e}`, "error");
  }
});

toggleHags.addEventListener("change", async () => {
  const enabled = toggleHags.checked;
  try {
    await invoke("set_hags", { enabled });
    setGamerStatus(
      `HAGS ${enabled ? "ativado" : "desativado"}. Reinicie o PC para aplicar.`,
      "success"
    );
  } catch (e) {
    toggleHags.checked = !enabled;
    setGamerStatus(`Erro: ${e}`, "error");
  }
});

loadPowerPlans();
loadGamerStatus();

// ---------- SFC / DISM ----------

const repairStatusEl = document.querySelector("#repair-status");
const repairListEl = document.querySelector("#repair-list");
const btnSfc = document.querySelector("#btn-sfc");
const btnDism = document.querySelector("#btn-dism");

function setRepairStatus(text, kind = "") {
  repairStatusEl.textContent = text;
  repairStatusEl.className = "status" + (kind ? ` ${kind}` : "");
}

async function renderRecentRepairs() {
  try {
    const repairs = await invoke("get_recent_sfc_repairs");
    repairListEl.innerHTML = "";
    if (repairs.length === 0) {
      const li = document.createElement("li");
      li.textContent = "Nenhum reparo recente encontrado no log.";
      repairListEl.appendChild(li);
      return;
    }
    for (const line of repairs) {
      const li = document.createElement("li");
      li.textContent = line;
      repairListEl.appendChild(li);
    }
  } catch (e) {
    setRepairStatus(`Erro ao ler CBS.log: ${e}`, "error");
  }
}

btnSfc.addEventListener("click", async () => {
  btnSfc.disabled = true;
  btnDism.disabled = true;
  setRepairStatus(
    "Rodando sfc /scannow... uma janela de permissão de administrador pode aparecer. Isso pode levar alguns minutos."
  );
  try {
    const msg = await invoke("run_sfc_scan");
    setRepairStatus(msg, "success");
    await renderRecentRepairs();
  } catch (e) {
    setRepairStatus(`Erro: ${e}`, "error");
  } finally {
    btnSfc.disabled = false;
    btnDism.disabled = false;
  }
});

btnDism.addEventListener("click", async () => {
  btnSfc.disabled = true;
  btnDism.disabled = true;
  setRepairStatus(
    "Rodando DISM RestoreHealth... isso pode levar vários minutos e precisa de internet."
  );
  try {
    const msg = await invoke("run_dism_restore_health");
    setRepairStatus(msg, "success");
  } catch (e) {
    setRepairStatus(`Erro: ${e}`, "error");
  } finally {
    btnSfc.disabled = false;
    btnDism.disabled = false;
  }
});

document.querySelector("#btn-repairs").addEventListener("click", renderRecentRepairs);

// ---------- Dispositivos ----------

const deviceListEl = document.querySelector("#device-list");
const deviceStatusEl = document.querySelector("#device-status");
const onlyErrorsEl = document.querySelector("#only-errors");
let devices = [];

function setDeviceStatus(text, kind = "") {
  deviceStatusEl.textContent = text;
  deviceStatusEl.className = "status" + (kind ? ` ${kind}` : "");
}

function renderDevices() {
  const onlyErrors = onlyErrorsEl.checked;
  const filtered = onlyErrors ? devices.filter((d) => d.has_error) : devices;
  deviceListEl.innerHTML = "";
  for (const d of filtered) {
    const li = document.createElement("li");
    li.className = "category-item";

    const left = document.createElement("div");
    const name = document.createElement("div");
    name.className = "category-name";
    name.textContent = d.name;
    const cls = document.createElement("div");
    cls.className = "category-size";
    cls.textContent = d.class;
    left.appendChild(name);
    left.appendChild(cls);

    const badge = document.createElement("span");
    badge.className = "status-badge " + (d.has_error ? "bad" : "good");
    badge.textContent = d.status;

    li.appendChild(left);
    li.appendChild(badge);
    deviceListEl.appendChild(li);
  }
  const errorCount = devices.filter((d) => d.has_error).length;
  setDeviceStatus(
    `${devices.length} dispositivos encontrados, ${errorCount} com erro.`,
    errorCount > 0 ? "warn" : "success"
  );
}

async function scanDevices() {
  setDeviceStatus("Verificando dispositivos...");
  try {
    devices = await invoke("list_devices");
    renderDevices();
  } catch (e) {
    setDeviceStatus(`Erro: ${e}`, "error");
  }
}

document.querySelector("#btn-scan-devices").addEventListener("click", scanDevices);
onlyErrorsEl.addEventListener("change", renderDevices);
document.querySelector("#btn-open-devmgmt").addEventListener("click", () => {
  invoke("open_device_manager").catch((e) => setDeviceStatus(`Erro: ${e}`, "error"));
});

scanDevices();

let usbDebounceTimer = null;
window.__TAURI__.event.listen("usb-changed", () => {
  clearTimeout(usbDebounceTimer);
  usbDebounceTimer = setTimeout(scanDevices, 500);
});

scan();
