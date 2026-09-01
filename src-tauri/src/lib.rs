use serde::Serialize;
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use winreg::enums::*;
use winreg::{RegKey, RegValue, HKEY};

struct UsbWatcherState(Mutex<Option<Child>>);
struct PerfWatcherState(Mutex<Option<Child>>);

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

enum CategoryKind {
    Folders(Vec<PathBuf>),
    ThumbnailCache(PathBuf),
    RecycleBin,
}

struct CategoryDef {
    id: &'static str,
    label: &'static str,
    kind: CategoryKind,
}

#[derive(Serialize)]
struct CategoryInfo {
    id: String,
    label: String,
    size_bytes: u64,
    size_human: String,
    exists: bool,
}

#[derive(Serialize)]
struct CleanOutcome {
    id: String,
    freed_bytes: u64,
    freed_human: String,
    error: Option<String>,
}

fn run_hidden(cmd: &str, args: &[&str]) -> std::io::Result<std::process::Output> {
    let mut c = Command::new(cmd);
    c.args(args);
    #[cfg(windows)]
    c.creation_flags(CREATE_NO_WINDOW);
    c.output()
}

fn category_defs() -> Vec<CategoryDef> {
    let temp = std::env::temp_dir();
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    let localapp = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let programdata = std::env::var("PROGRAMDATA").unwrap_or_else(|_| "C:\\ProgramData".into());

    vec![
        CategoryDef {
            id: "temp_user",
            label: "Arquivos temporários do usuário",
            kind: CategoryKind::Folders(vec![temp]),
        },
        CategoryDef {
            id: "temp_windows",
            label: "Temp do Windows",
            kind: CategoryKind::Folders(vec![PathBuf::from(format!("{windir}\\Temp"))]),
        },
        CategoryDef {
            id: "wer",
            label: "Relatórios de erro do Windows",
            kind: CategoryKind::Folders(vec![
                PathBuf::from(format!("{programdata}\\Microsoft\\Windows\\WER\\ReportQueue")),
                PathBuf::from(format!("{programdata}\\Microsoft\\Windows\\WER\\ReportArchive")),
            ]),
        },
        CategoryDef {
            id: "thumbnail_cache",
            label: "Cache de miniaturas",
            kind: CategoryKind::ThumbnailCache(PathBuf::from(format!(
                "{localapp}\\Microsoft\\Windows\\Explorer"
            ))),
        },
        CategoryDef {
            id: "inetcache",
            label: "Cache de Internet (INetCache)",
            kind: CategoryKind::Folders(vec![PathBuf::from(format!(
                "{localapp}\\Microsoft\\Windows\\INetCache"
            ))]),
        },
        CategoryDef {
            id: "chrome_cache",
            label: "Cache do Chrome",
            kind: CategoryKind::Folders(vec![
                PathBuf::from(format!("{localapp}\\Google\\Chrome\\User Data\\Default\\Cache")),
                PathBuf::from(format!(
                    "{localapp}\\Google\\Chrome\\User Data\\Default\\Code Cache"
                )),
            ]),
        },
        CategoryDef {
            id: "edge_cache",
            label: "Cache do Edge",
            kind: CategoryKind::Folders(vec![
                PathBuf::from(format!("{localapp}\\Microsoft\\Edge\\User Data\\Default\\Cache")),
                PathBuf::from(format!(
                    "{localapp}\\Microsoft\\Edge\\User Data\\Default\\Code Cache"
                )),
            ]),
        },
        CategoryDef {
            id: "windows_update",
            label: "Cache do Windows Update",
            kind: CategoryKind::Folders(vec![PathBuf::from(format!(
                "{windir}\\SoftwareDistribution\\Download"
            ))]),
        },
        CategoryDef {
            id: "delivery_optimization",
            label: "Cache de Delivery Optimization",
            kind: CategoryKind::Folders(vec![PathBuf::from(format!(
                "{windir}\\SoftwareDistribution\\DeliveryOptimization"
            ))]),
        },
        CategoryDef {
            id: "recycle_bin",
            label: "Lixeira",
            kind: CategoryKind::RecycleBin,
        },
    ]
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                if meta.is_dir() {
                    total += dir_size(&entry.path());
                } else {
                    total += meta.len();
                }
            }
        }
    }
    total
}

fn clear_dir_contents(path: &Path) -> u64 {
    let mut freed = 0u64;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if let Ok(meta) = entry.metadata() {
                let size = if meta.is_dir() { dir_size(&p) } else { meta.len() };
                let removed = if meta.is_dir() {
                    fs::remove_dir_all(&p).is_ok()
                } else {
                    fs::remove_file(&p).is_ok()
                };
                if removed {
                    freed += size;
                }
            }
        }
    }
    freed
}

fn thumbnail_files(path: &Path) -> Vec<PathBuf> {
    let mut v = vec![];
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name.starts_with("thumbcache_") || name.starts_with("iconcache_") {
                v.push(entry.path());
            }
        }
    }
    v
}

fn human_size(bytes: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut i = 0;
    while size >= 1024.0 && i < units.len() - 1 {
        size /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{} {}", bytes, units[i])
    } else {
        format!("{:.2} {}", size, units[i])
    }
}

fn recycle_bin_size() -> u64 {
    if let Ok(out) = run_hidden(
        "powershell",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(New-Object -ComObject Shell.Application).Namespace(10).Items() | Measure-Object -Property Size -Sum | Select-Object -ExpandProperty Sum",
        ],
    ) {
        let s = String::from_utf8_lossy(&out.stdout);
        s.trim().parse::<u64>().unwrap_or(0)
    } else {
        0
    }
}

fn empty_recycle_bin() -> Result<(), String> {
    match run_hidden(
        "powershell",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Clear-RecycleBin -Force -ErrorAction Stop",
        ],
    ) {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => Err(String::from_utf8_lossy(&out.stderr).trim().to_string()),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
fn scan_categories() -> Vec<CategoryInfo> {
    category_defs()
        .into_iter()
        .map(|def| {
            let (size, exists) = match &def.kind {
                CategoryKind::Folders(paths) => {
                    let mut total = 0u64;
                    let mut any_exists = false;
                    for p in paths {
                        if p.exists() {
                            any_exists = true;
                            total += dir_size(p);
                        }
                    }
                    (total, any_exists)
                }
                CategoryKind::ThumbnailCache(p) => {
                    if p.exists() {
                        let files = thumbnail_files(p);
                        let total: u64 = files
                            .iter()
                            .filter_map(|f| fs::metadata(f).ok())
                            .map(|m| m.len())
                            .sum();
                        (total, true)
                    } else {
                        (0, false)
                    }
                }
                CategoryKind::RecycleBin => (recycle_bin_size(), true),
            };
            CategoryInfo {
                id: def.id.into(),
                label: def.label.into(),
                size_bytes: size,
                size_human: human_size(size),
                exists,
            }
        })
        .collect()
}

#[tauri::command]
fn clean_categories(ids: Vec<String>) -> Vec<CleanOutcome> {
    let defs = category_defs();
    ids.iter()
        .map(|id| {
            let def = defs.iter().find(|d| d.id == id);
            match def {
                None => CleanOutcome {
                    id: id.clone(),
                    freed_bytes: 0,
                    freed_human: human_size(0),
                    error: Some("categoria desconhecida".into()),
                },
                Some(def) => match &def.kind {
                    CategoryKind::Folders(paths) => {
                        let mut freed = 0u64;
                        for p in paths {
                            if p.exists() {
                                freed += clear_dir_contents(p);
                            }
                        }
                        CleanOutcome {
                            id: id.clone(),
                            freed_bytes: freed,
                            freed_human: human_size(freed),
                            error: None,
                        }
                    }
                    CategoryKind::ThumbnailCache(p) => {
                        let mut freed = 0u64;
                        if p.exists() {
                            for f in thumbnail_files(p) {
                                if let Ok(meta) = fs::metadata(&f) {
                                    if fs::remove_file(&f).is_ok() {
                                        freed += meta.len();
                                    }
                                }
                            }
                        }
                        CleanOutcome {
                            id: id.clone(),
                            freed_bytes: freed,
                            freed_human: human_size(freed),
                            error: None,
                        }
                    }
                    CategoryKind::RecycleBin => {
                        let before = recycle_bin_size();
                        match empty_recycle_bin() {
                            Ok(_) => CleanOutcome {
                                id: id.clone(),
                                freed_bytes: before,
                                freed_human: human_size(before),
                                error: None,
                            },
                            Err(e) => CleanOutcome {
                                id: id.clone(),
                                freed_bytes: 0,
                                freed_human: human_size(0),
                                error: Some(e),
                            },
                        }
                    }
                },
            }
        })
        .collect()
}

#[tauri::command]
fn flush_dns() -> Result<String, String> {
    match run_hidden("ipconfig", &["/flushdns"]) {
        Ok(out) if out.status.success() => Ok("Cache de DNS limpo com sucesso.".into()),
        Ok(out) => Err(String::from_utf8_lossy(&out.stderr).trim().to_string()),
        Err(e) => Err(e.to_string()),
    }
}

// ---------- Otimizações para gamers ----------

#[derive(Serialize)]
struct PowerPlan {
    guid: String,
    name: String,
    active: bool,
}

#[derive(Serialize)]
struct GamerStatus {
    game_mode: bool,
    game_dvr: bool,
    hags: Option<bool>,
    is_admin: bool,
}

const ULTIMATE_PERFORMANCE_GUID: &str = "e9a42b02-d5df-448d-aa00-03f14749eb61";

fn extract_guid(text: &str) -> Option<String> {
    text.split_whitespace().find_map(|tok| {
        let t = tok.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-');
        if t.len() == 36 && t.chars().filter(|&c| c == '-').count() == 4 {
            Some(t.to_string())
        } else {
            None
        }
    })
}

fn powercfg_list() -> Result<Vec<PowerPlan>, String> {
    let out = run_hidden("powercfg", &["/list"]).map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut plans = vec![];
    for line in text.lines() {
        if let Some(guid_pos) = line.find("GUID:") {
            let rest = line[guid_pos + 5..].trim();
            if let Some(guid_end) = rest.find(char::is_whitespace) {
                let guid = rest[..guid_end].to_string();
                let after = rest[guid_end..].trim_start();
                let name = after
                    .trim_start_matches('(')
                    .split(')')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let active = after.contains('*');
                plans.push(PowerPlan { guid, name, active });
            }
        }
    }
    Ok(plans)
}

#[tauri::command]
fn list_power_plans() -> Result<Vec<PowerPlan>, String> {
    powercfg_list()
}

#[tauri::command]
fn set_power_plan(guid: String) -> Result<(), String> {
    let out = run_hidden("powercfg", &["/setactive", &guid]).map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[tauri::command]
fn enable_ultimate_performance() -> Result<Vec<PowerPlan>, String> {
    let plans = powercfg_list()?;
    let existing = plans.iter().find(|p| {
        let n = p.name.to_lowercase();
        n.contains("desempenho máximo")
            || n.contains("desempenho maximo")
            || n.contains("ultimate performance")
    });
    let target_guid = if let Some(p) = existing {
        p.guid.clone()
    } else {
        let out = run_hidden("powercfg", &["-duplicatescheme", ULTIMATE_PERFORMANCE_GUID])
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        let text = String::from_utf8_lossy(&out.stdout);
        extract_guid(&text).ok_or_else(|| "Não foi possível identificar o novo esquema criado.".to_string())?
    };
    set_power_plan(target_guid)?;
    powercfg_list()
}

fn get_dword_hkcu(path: &str, name: &str) -> Option<u32> {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(path)
        .ok()?
        .get_value(name)
        .ok()
}

fn set_dword_hkcu(path: &str, name: &str, value: u32) -> Result<(), String> {
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey(path)
        .map_err(|e| e.to_string())?;
    key.set_value(name, &value).map_err(|e| e.to_string())
}

fn get_dword_hklm(path: &str, name: &str) -> Option<u32> {
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(path)
        .ok()?
        .get_value(name)
        .ok()
}

fn set_dword_hklm(path: &str, name: &str, value: u32) -> Result<(), String> {
    let (key, _) = RegKey::predef(HKEY_LOCAL_MACHINE)
        .create_subkey(path)
        .map_err(|_| "Permissão negada. Execute o aplicativo como Administrador para essa opção.".to_string())?;
    key.set_value(name, &value).map_err(|e| e.to_string())
}

fn is_admin() -> bool {
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(
            "SYSTEM\\CurrentControlSet\\Control\\GraphicsDrivers",
            KEY_SET_VALUE,
        )
        .is_ok()
}

#[tauri::command]
fn get_gamer_status() -> GamerStatus {
    let game_mode = get_dword_hkcu("Software\\Microsoft\\GameBar", "AutoGameModeEnabled")
        .map(|v| v != 0)
        .unwrap_or(true);
    let game_dvr = get_dword_hkcu("System\\GameConfigStore", "GameDVR_Enabled")
        .map(|v| v != 0)
        .unwrap_or(true);
    let hags = get_dword_hklm(
        "SYSTEM\\CurrentControlSet\\Control\\GraphicsDrivers",
        "HwSchMode",
    )
    .map(|v| v == 2);
    GamerStatus {
        game_mode,
        game_dvr,
        hags,
        is_admin: is_admin(),
    }
}

#[tauri::command]
fn set_game_mode(enabled: bool) -> Result<(), String> {
    set_dword_hkcu(
        "Software\\Microsoft\\GameBar",
        "AutoGameModeEnabled",
        enabled as u32,
    )
}

#[tauri::command]
fn set_game_dvr(enabled: bool) -> Result<(), String> {
    set_dword_hkcu("System\\GameConfigStore", "GameDVR_Enabled", enabled as u32)
}

#[tauri::command]
fn set_hags(enabled: bool) -> Result<(), String> {
    set_dword_hklm(
        "SYSTEM\\CurrentControlSet\\Control\\GraphicsDrivers",
        "HwSchMode",
        if enabled { 2 } else { 1 },
    )
}

// ---------- SFC / DISM ----------

fn run_elevated_and_wait(exe: &str, args: &[&str]) -> Result<i32, String> {
    let args_ps = args
        .iter()
        .map(|a| format!("'{}'", a.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(",");
    let script = format!(
        "try {{ $p = Start-Process -FilePath '{exe}' -ArgumentList {args_ps} -Verb RunAs -Wait -PassThru -ErrorAction Stop; exit $p.ExitCode }} catch {{ exit 1223 }}"
    );
    let out = run_hidden(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", &script],
    )
    .map_err(|e| e.to_string())?;
    Ok(out.status.code().unwrap_or(-1))
}

fn run_elevated_capture(exe: &str, args: &[&str]) -> Result<String, String> {
    let out_file = std::env::temp_dir().join(format!("pccleaner_{}.txt", std::process::id()));
    let _ = fs::remove_file(&out_file);
    let out_path = out_file.to_string_lossy().replace('\'', "''");
    let args_ps = args
        .iter()
        .map(|a| format!("'{}'", a.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(",");
    let script = format!(
        "try {{ $p = Start-Process -FilePath '{exe}' -ArgumentList {args_ps} -Verb RunAs -Wait -PassThru -WindowStyle Hidden -RedirectStandardOutput '{out_path}' -ErrorAction Stop; exit $p.ExitCode }} catch {{ exit 1223 }}"
    );
    let out = run_hidden(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", &script],
    )
    .map_err(|e| e.to_string())?;
    let code = out.status.code().unwrap_or(-1);
    let content = fs::read_to_string(&out_file).unwrap_or_default();
    let _ = fs::remove_file(&out_file);
    if code == 1223 {
        return Err("Permissão de administrador negada.".into());
    }
    Ok(content)
}

#[tauri::command]
fn run_sfc_scan() -> Result<String, String> {
    let code = run_elevated_and_wait("sfc.exe", &["/scannow"])?;
    if code == 1223 {
        return Err("Permissão de administrador negada.".into());
    }
    Ok(format!(
        "sfc /scannow concluído (código {code}). Veja o resumo de reparos abaixo."
    ))
}

#[tauri::command]
fn run_dism_restore_health() -> Result<String, String> {
    let code = run_elevated_and_wait(
        "DISM.exe",
        &["/Online", "/Cleanup-Image", "/RestoreHealth"],
    )?;
    if code == 1223 {
        return Err("Permissão de administrador negada.".into());
    }
    if code == 0 {
        Ok("DISM concluído com êxito: a imagem do sistema está saudável.".into())
    } else {
        Ok(format!(
            "DISM terminou com código {code}. Pode ser necessário rodar de novo ou verificar a conexão com a internet."
        ))
    }
}

// ---------- Dispositivos e drivers ----------

#[derive(serde::Deserialize)]
struct RawDevice {
    #[serde(rename = "FriendlyName")]
    friendly_name: Option<String>,
    #[serde(rename = "Class")]
    class: Option<String>,
    #[serde(rename = "Status")]
    status: Option<String>,
}

#[derive(Serialize)]
struct DeviceInfo {
    name: String,
    class: String,
    status: String,
    has_error: bool,
}

#[tauri::command]
fn list_devices() -> Result<Vec<DeviceInfo>, String> {
    let script = "$d = @(Get-PnpDevice -PresentOnly | Where-Object { $_.InstanceId -match '^USB\\\\|^HID\\\\' } | Select-Object FriendlyName,Class,Status); ConvertTo-Json -InputObject $d -Compress";
    let out = run_hidden(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", script],
    )
    .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(vec![]);
    }
    let raw: Vec<RawDevice> = serde_json::from_str(trimmed).map_err(|e| e.to_string())?;
    let devices = raw
        .into_iter()
        .map(|d| {
            let status = d.status.unwrap_or_else(|| "Unknown".into());
            let has_error = status.eq_ignore_ascii_case("Error");
            DeviceInfo {
                name: d.friendly_name.unwrap_or_else(|| "(sem nome)".into()),
                class: d.class.unwrap_or_else(|| "Outro".into()),
                status,
                has_error,
            }
        })
        .collect();
    Ok(devices)
}

#[tauri::command]
fn open_device_manager() -> Result<(), String> {
    Command::new("mmc")
        .arg("devmgmt.msc")
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_recent_sfc_repairs() -> Result<Vec<String>, String> {
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    let path = format!("{windir}\\Logs\\CBS\\CBS.log");
    let mut file = fs::File::open(&path).map_err(|e| e.to_string())?;
    let len = file.metadata().map_err(|e| e.to_string())?.len();
    let read_size = 3_000_000u64.min(len);
    file.seek(SeekFrom::End(-(read_size as i64)))
        .map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&buf);
    let repairs: Vec<String> = text
        .lines()
        .filter(|l| l.contains("[SR] Repairing file") || l.contains("[SR] Cannot repair"))
        .map(|s| s.trim().to_string())
        .collect();
    Ok(repairs)
}

// ---------- Espaço profundo (Windows.old, hibernação, WinSxS) ----------

#[derive(Serialize)]
struct DeepSpaceInfo {
    windows_old_exists: bool,
    windows_old_size_human: String,
    hibernation_enabled: bool,
    hiberfil_size_human: String,
}

fn system_drive() -> String {
    std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into())
}

#[tauri::command]
fn get_deep_space_info() -> DeepSpaceInfo {
    let drive = system_drive();
    let windows_old = format!("{drive}\\Windows.old");
    let windows_old_exists = Path::new(&windows_old).exists();
    let windows_old_size = if windows_old_exists {
        dir_size(Path::new(&windows_old))
    } else {
        0
    };

    let hiber_path = format!("{drive}\\hiberfil.sys");
    let hiber_meta = fs::metadata(&hiber_path);
    let hibernation_enabled = hiber_meta.is_ok();
    let hiber_size = hiber_meta.map(|m| m.len()).unwrap_or(0);

    DeepSpaceInfo {
        windows_old_exists,
        windows_old_size_human: human_size(windows_old_size),
        hibernation_enabled,
        hiberfil_size_human: human_size(hiber_size),
    }
}

#[tauri::command]
fn remove_windows_old() -> Result<String, String> {
    let path = format!("{}\\Windows.old", system_drive());
    if !Path::new(&path).exists() {
        return Ok("Pasta Windows.old não encontrada.".into());
    }
    let inner = format!(
        "takeown /F \"{path}\" /R /D Y | Out-Null; icacls \"{path}\" /grant *S-1-5-32-544:F /T /C | Out-Null; Remove-Item -LiteralPath \"{path}\" -Recurse -Force -ErrorAction SilentlyContinue; if (Test-Path \"{path}\") {{ 'RESTOU' }} else {{ 'REMOVIDO' }}"
    );
    let out = run_elevated_capture("powershell.exe", &["-NoProfile", "-Command", &inner])?;
    if out.contains("REMOVIDO") {
        Ok("Windows.old removido com sucesso.".into())
    } else {
        Ok("Alguns arquivos não puderam ser removidos (podem estar em uso). Tente reiniciar o PC e rodar de novo.".into())
    }
}

#[tauri::command]
fn set_hibernation(enabled: bool) -> Result<String, String> {
    let flag = if enabled { "on" } else { "off" };
    run_elevated_capture("powercfg.exe", &["/h", flag])?;
    Ok(format!(
        "Hibernação {}.",
        if enabled { "ativada" } else { "desativada" }
    ))
}

#[tauri::command]
fn analyze_component_store() -> Result<String, String> {
    run_elevated_capture(
        "DISM.exe",
        &["/Online", "/Cleanup-Image", "/AnalyzeComponentStore"],
    )
}

#[tauri::command]
fn clean_component_store() -> Result<String, String> {
    run_elevated_capture(
        "DISM.exe",
        &["/Online", "/Cleanup-Image", "/StartComponentCleanup"],
    )
}

// ---------- Inicialização ----------

#[derive(Serialize)]
struct StartupItem {
    id: String,
    name: String,
    source: String,
    command: String,
    enabled: bool,
}

fn is_run_entry_enabled(root: HKEY, name: &str) -> bool {
    let key = RegKey::predef(root);
    match key.open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run") {
        Ok(approved) => match approved.get_raw_value(name) {
            Ok(v) => v.bytes.first().copied() != Some(3),
            Err(_) => true,
        },
        Err(_) => true,
    }
}

fn set_startup_approved(root: HKEY, name: &str, enabled: bool) -> Result<(), String> {
    let hive = RegKey::predef(root);
    let (key, _) = hive
        .create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run")
        .map_err(|_| "Permissão negada. Execute como Administrador para essa opção.".to_string())?;
    let mut bytes = key.get_raw_value(name).map(|v| v.bytes).unwrap_or_default();
    if bytes.len() < 1 {
        bytes = vec![0u8; 12];
    }
    bytes[0] = if enabled { 2 } else { 3 };
    key.set_raw_value(
        name,
        &RegValue {
            bytes,
            vtype: REG_BINARY,
        },
    )
    .map_err(|e| e.to_string())
}

fn list_run_key_items(root: HKEY, hive_label: &str, source_label: &str) -> Vec<StartupItem> {
    let mut items = vec![];
    let key = RegKey::predef(root);
    if let Ok(run) = key.open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run") {
        for (name, value) in run.enum_values().filter_map(Result::ok) {
            items.push(StartupItem {
                id: format!("reg|{hive_label}|{name}"),
                name: name.clone(),
                source: source_label.to_string(),
                command: value.to_string(),
                enabled: is_run_entry_enabled(root, &name),
            });
        }
    }
    items
}

fn list_startup_folder_items(dir: &Path, source_label: &str) -> Vec<StartupItem> {
    let mut items = vec![];
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let full = path.to_string_lossy().to_string();
                let file_name = entry.file_name().to_string_lossy().to_string();
                let (display_name, enabled) = match file_name.strip_suffix(".disabled") {
                    Some(stripped) => (stripped.to_string(), false),
                    None => (file_name.clone(), true),
                };
                items.push(StartupItem {
                    id: format!("folder|{full}"),
                    name: display_name.trim_end_matches(".lnk").to_string(),
                    source: source_label.to_string(),
                    command: full,
                    enabled,
                });
            }
        }
    }
    items
}

#[derive(serde::Deserialize)]
struct RawTask {
    #[serde(rename = "TaskName")]
    task_name: String,
    #[serde(rename = "TaskPath")]
    task_path: String,
    #[serde(rename = "State")]
    state: String,
}

#[tauri::command]
fn list_startup_items() -> Result<Vec<StartupItem>, String> {
    let mut items = vec![];
    items.extend(list_run_key_items(
        HKEY_CURRENT_USER,
        "HKCU",
        "Registro (usuário)",
    ));
    items.extend(list_run_key_items(
        HKEY_LOCAL_MACHINE,
        "HKLM",
        "Registro (máquina)",
    ));

    let appdata = std::env::var("APPDATA").unwrap_or_default();
    let programdata = std::env::var("PROGRAMDATA").unwrap_or_else(|_| "C:\\ProgramData".into());
    items.extend(list_startup_folder_items(
        &PathBuf::from(format!(
            "{appdata}\\Microsoft\\Windows\\Start Menu\\Programs\\Startup"
        )),
        "Pasta Startup (usuário)",
    ));
    items.extend(list_startup_folder_items(
        &PathBuf::from(format!(
            "{programdata}\\Microsoft\\Windows\\Start Menu\\Programs\\Startup"
        )),
        "Pasta Startup (todos)",
    ));

    let script = "$t = @(Get-ScheduledTask | Where-Object { ($_.Triggers | ForEach-Object { $_.CimClass.CimClassName }) -contains 'MSFT_TaskLogonTrigger' } | Select-Object TaskName,TaskPath,State); ConvertTo-Json -InputObject $t -Compress";
    if let Ok(out) = run_hidden(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", script],
    ) {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                if let Ok(tasks) = serde_json::from_str::<Vec<RawTask>>(trimmed) {
                    for t in tasks {
                        items.push(StartupItem {
                            id: format!("task|{}|{}", t.task_path, t.task_name),
                            name: t.task_name,
                            source: "Tarefa agendada".into(),
                            command: t.task_path,
                            enabled: t.state != "Disabled",
                        });
                    }
                }
            }
        }
    }

    Ok(items)
}

#[tauri::command]
fn set_startup_item_enabled(id: String, enabled: bool) -> Result<(), String> {
    let parts: Vec<&str> = id.splitn(3, '|').collect();
    match parts.as_slice() {
        ["reg", hive, name] => {
            let root = if *hive == "HKCU" {
                HKEY_CURRENT_USER
            } else {
                HKEY_LOCAL_MACHINE
            };
            set_startup_approved(root, name, enabled)
        }
        ["folder", path, ..] => {
            let p = Path::new(path);
            if enabled {
                if let Some(stripped) = path.strip_suffix(".disabled") {
                    fs::rename(p, stripped).map_err(|e| e.to_string())?;
                }
            } else if !path.ends_with(".disabled") {
                fs::rename(p, format!("{path}.disabled")).map_err(|e| e.to_string())?;
            }
            Ok(())
        }
        ["task", task_path, task_name] => {
            let action = if enabled {
                "Enable-ScheduledTask"
            } else {
                "Disable-ScheduledTask"
            };
            let script = format!(
                "{action} -TaskName '{}' -TaskPath '{}'",
                task_name.replace('\'', "''"),
                task_path.replace('\'', "''")
            );
            run_elevated_capture("powershell.exe", &["-NoProfile", "-Command", &script])?;
            Ok(())
        }
        _ => Err("id inválido".into()),
    }
}

// ---------- Confiabilidade ----------

#[derive(serde::Deserialize)]
struct RawReliabilityEvent {
    #[serde(rename = "Id")]
    id: u32,
    #[serde(rename = "LogName")]
    log_name: String,
    #[serde(rename = "Time")]
    time: String,
    #[serde(rename = "Msg")]
    msg: Option<String>,
}

#[derive(Serialize)]
struct ReliabilityEvent {
    time: String,
    kind: String,
    description: String,
}

#[derive(Serialize)]
struct BootInfo {
    last_boot: String,
    uptime_days: i32,
    uptime_hours: i32,
}

fn event_kind_label(log_name: &str, id: u32) -> &'static str {
    match (log_name, id) {
        ("System", 41) => "Desligamento inesperado (queda de energia ou falha)",
        ("System", 6008) => "Desligamento inesperado",
        ("System", 1001) => "Tela azul (BSOD)",
        ("System", 7) | ("System", 51) | ("System", 153) => "Erro de disco",
        ("Application", 1000) => "Travamento de aplicativo",
        ("Application", 1002) => "Aplicativo não respondeu (hang)",
        _ => "Evento do sistema",
    }
}

#[tauri::command]
fn get_reliability_events() -> Result<Vec<ReliabilityEvent>, String> {
    let script = r#"
        $start = (Get-Date).AddDays(-30)
        $events = @()
        try {
            $events += Get-WinEvent -FilterHashtable @{LogName='System';Id=41,1001,6008,7,51,153;StartTime=$start} -ErrorAction Stop |
                Select-Object @{n='Id';e={$_.Id}}, @{n='LogName';e={'System'}}, @{n='Time';e={$_.TimeCreated.ToString('yyyy-MM-dd HH:mm:ss')}}, @{n='Msg';e={($_.Message -split "`n")[0]}}
        } catch {}
        try {
            $events += Get-WinEvent -FilterHashtable @{LogName='Application';Id=1000,1002;StartTime=$start} -ErrorAction Stop |
                Select-Object @{n='Id';e={$_.Id}}, @{n='LogName';e={'Application'}}, @{n='Time';e={$_.TimeCreated.ToString('yyyy-MM-dd HH:mm:ss')}}, @{n='Msg';e={($_.Message -split "`n")[0]}}
        } catch {}
        $events = $events | Sort-Object Time -Descending | Select-Object -First 60
        ConvertTo-Json -InputObject @($events) -Compress
    "#;
    let out = run_hidden(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", script],
    )
    .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(vec![]);
    }
    let raw: Vec<RawReliabilityEvent> = serde_json::from_str(trimmed).map_err(|e| e.to_string())?;
    Ok(raw
        .into_iter()
        .map(|e| ReliabilityEvent {
            time: e.time,
            kind: event_kind_label(&e.log_name, e.id).to_string(),
            description: e.msg.unwrap_or_default(),
        })
        .collect())
}

#[tauri::command]
fn get_boot_info() -> Result<BootInfo, String> {
    #[derive(serde::Deserialize)]
    struct RawBoot {
        #[serde(rename = "Boot")]
        boot: String,
        #[serde(rename = "Days")]
        days: i32,
        #[serde(rename = "Hours")]
        hours: i32,
    }
    let script = r#"
        $os = Get-CimInstance Win32_OperatingSystem
        $boot = $os.LastBootUpTime
        $uptime = (Get-Date) - $boot
        [PSCustomObject]@{ Boot = $boot.ToString('yyyy-MM-dd HH:mm:ss'); Days = [int]$uptime.Days; Hours = $uptime.Hours } | ConvertTo-Json -Compress
    "#;
    let out = run_hidden(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", script],
    )
    .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let raw: RawBoot = serde_json::from_str(text.trim()).map_err(|e| e.to_string())?;
    Ok(BootInfo {
        last_boot: raw.boot,
        uptime_days: raw.days,
        uptime_hours: raw.hours,
    })
}

// ---------- Desempenho ----------

#[tauri::command]
fn start_perf_monitor(
    app: tauri::AppHandle,
    state: tauri::State<PerfWatcherState>,
) -> Result<(), String> {
    let mut guard = state.0.lock().map_err(|e| e.to_string())?;
    if guard.is_some() {
        return Ok(());
    }
    let script = r#"
        while ($true) {
            try {
                $cpu = (Get-CimInstance Win32_PerfFormattedData_PerfOS_Processor -Filter "Name='_Total'").PercentProcessorTime
                $os = Get-CimInstance Win32_OperatingSystem
                $memTotal = $os.TotalVisibleMemorySize
                $memFree = $os.FreePhysicalMemory
                $memUsedPct = [math]::Round((($memTotal - $memFree) / $memTotal) * 100, 1)
                $disk = (Get-CimInstance Win32_PerfFormattedData_PerfDisk_PhysicalDisk -Filter "Name='_Total'").PercentDiskTime
                $top = Get-Process | Sort-Object WorkingSet64 -Descending | Select-Object -First 8 -Property @{n='Name';e={$_.ProcessName}}, @{n='Pid';e={$_.Id}}, @{n='MemMB';e={[math]::Round($_.WorkingSet64/1MB,1)}}, @{n='CpuS';e={[math]::Round($_.CPU,1)}}
                $out = [PSCustomObject]@{
                    CpuPct = $cpu
                    MemUsedPct = $memUsedPct
                    MemUsedMB = [math]::Round(($memTotal-$memFree)/1024,0)
                    MemTotalMB = [math]::Round($memTotal/1024,0)
                    DiskPct = $disk
                    Top = $top
                }
                $out | ConvertTo-Json -Compress -Depth 3
            } catch {}
            Start-Sleep -Milliseconds 1500
        }
    "#;
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", script])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let stdout = child.stdout.take().ok_or("sem stdout")?;
    *guard = Some(child);
    drop(guard);

    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(l) => {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&l) {
                        let _ = app.emit("perf-tick", value);
                    }
                }
                Err(_) => break,
            }
        }
    });
    Ok(())
}

#[tauri::command]
fn stop_perf_monitor(state: tauri::State<PerfWatcherState>) -> Result<(), String> {
    let mut guard = state.0.lock().map_err(|e| e.to_string())?;
    if let Some(mut child) = guard.take() {
        let _ = child.kill();
    }
    Ok(())
}

fn start_usb_watcher(app: tauri::AppHandle) {
    let script = r#"
        $query = New-Object System.Management.WqlEventQuery("SELECT * FROM __InstanceOperationEvent WITHIN 1 WHERE TargetInstance ISA 'Win32_PnPEntity'")
        $watcher = New-Object System.Management.ManagementEventWatcher($query)
        while ($true) {
            $watcher.WaitForNextEvent() | Out-Null
            Write-Output "CHANGED"
        }
    "#;
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", script])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    match cmd.spawn() {
        Ok(mut child) => {
            if let Some(stdout) = child.stdout.take() {
                if let Some(state) = app.try_state::<UsbWatcherState>() {
                    if let Ok(mut g) = state.0.lock() {
                        *g = Some(child);
                    }
                }
                for line in BufReader::new(stdout).lines() {
                    if line.is_err() {
                        break;
                    }
                    let _ = app.emit("usb-changed", ());
                }
            }
        }
        Err(e) => eprintln!("Falha ao iniciar monitor de USB: {e}"),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(UsbWatcherState(Mutex::new(None)))
        .manage(PerfWatcherState(Mutex::new(None)))
        .setup(|app| {
            let handle = app.handle().clone();
            std::thread::spawn(move || start_usb_watcher(handle));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            scan_categories,
            clean_categories,
            flush_dns,
            list_power_plans,
            set_power_plan,
            enable_ultimate_performance,
            get_gamer_status,
            set_game_mode,
            set_game_dvr,
            set_hags,
            run_sfc_scan,
            run_dism_restore_health,
            get_recent_sfc_repairs,
            list_devices,
            open_device_manager,
            get_deep_space_info,
            remove_windows_old,
            set_hibernation,
            analyze_component_store,
            clean_component_store,
            list_startup_items,
            set_startup_item_enabled,
            get_reliability_events,
            get_boot_info,
            start_perf_monitor,
            stop_perf_monitor
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = app_handle.try_state::<UsbWatcherState>() {
                    if let Ok(mut g) = state.0.lock() {
                        if let Some(mut c) = g.take() {
                            let _ = c.kill();
                        }
                    }
                }
                if let Some(state) = app_handle.try_state::<PerfWatcherState>() {
                    if let Ok(mut g) = state.0.lock() {
                        if let Some(mut c) = g.take() {
                            let _ = c.kill();
                        }
                    }
                }
            }
        });
}
