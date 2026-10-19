"""Build an isolated Yuzu delay-slider host and native plugin from an existing build.

Reads --base build inputs; writes only this worktree's build/latency-slider.
No game code is recompiled, patched, or replaced. Generated game data stays ignored.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "build/latency-slider"
TARGET = 0x16CCAE0
TARGET_SIZE = 400
TARGET_SHA256 = "321ce8ab4a70fed64737950bdc8feac5b1eb258bfa39998b62db4fb3ebb98ecf"


def digest(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def verify_elf(path):
    with Path(path).open("rb") as stream:
        header = stream.read(64)
        if header[:6] != b"\x7fELF\x02\x01" or int.from_bytes(header[18:20], "little") != 183:
            raise ValueError("Expected a little-endian AArch64 ELF dump")
        stream.seek(0x1000 + TARGET)
        block = stream.read(TARGET_SIZE)
    if hashlib.sha256(block).hexdigest() != TARGET_SHA256:
        raise ValueError("Unsupported game calculator fingerprint; no experimental build produced")
    return TARGET_SHA256


def write_changed(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    if not path.exists() or path.read_bytes() != data:
        temporary = path.with_name(path.name + ".delay-stage.tmp")
        temporary.write_bytes(data)
        os.replace(temporary, path)


def clean_environment():
    return {k: v for k, v in os.environ.items() if not k.upper().startswith(("RECOMP_", "SSBU_"))}


def load_base(base):
    base = Path(base).resolve()
    state = json.loads((base / "build/state.json").read_text(encoding="utf-8-sig"))
    return base, state


def native_toolchain():
    # The game keeps its pinned clang. The independent extension uses the
    # installed MSVC Build Tools and exchanges only plain C records with it.
    vswhere = Path(os.environ.get("ProgramFiles(x86)", "C:/Program Files (x86)")) / "Microsoft Visual Studio/Installer/vswhere.exe"
    installation = Path(subprocess.check_output([str(vswhere), "-latest", "-products", "*", "-requires",
                         "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath"], text=True).strip())
    version = (installation / "VC/Auxiliary/Build/Microsoft.VCToolsVersion.default.txt").read_text().strip()
    tools = installation / "VC/Tools/MSVC" / version
    kits = Path(os.environ.get("ProgramFiles(x86)", "C:/Program Files (x86)")) / "Windows Kits/10"
    sdk = sorted(p for p in (kits / "Include").iterdir() if (p / "um/windows.h").is_file())[-1].name
    environment = clean_environment()
    environment["PATH"] = str(tools / "bin/Hostx64/x64") + os.pathsep + environment["PATH"]
    environment["INCLUDE"] = ";".join(map(str, [tools / "include", *[kits / "Include" / sdk / n for n in ("ucrt", "shared", "um", "winrt")]]))
    environment["LIB"] = ";".join(map(str, [tools / "lib/x64", kits / "Lib" / sdk / "ucrt/x64", kits / "Lib" / sdk / "um/x64"]))
    return str(tools / "bin/Hostx64/x64/cl.exe"), environment


def native_compile(sources, target, includes=(), libraries=(), shared=False, windows=False):
    compiler, environment = native_toolchain()
    objects = target.parent / (target.stem + "-objects")
    objects.mkdir(parents=True, exist_ok=True)
    command = [compiler, "/nologo", "/std:c++17", "/O2", "/EHsc", "/MD", "/utf-8", "/W3",
               "/D_CRT_SECURE_NO_WARNINGS", "/Fo" + str(objects) + "\\", "/Fe" + str(target)]
    if shared:
        command.append("/LD")
    command.extend("/I" + str(path) for path in includes)
    command.extend(map(str, sources))
    command.append("/link")
    if windows:
        command.append("/SUBSYSTEM:WINDOWS")
    command.extend(map(str, libraries))
    subprocess.run(command, check=True, env=environment)


def compile_plugin(clang, plugins=None):
    plugins = Path(plugins) if plugins else OUT / "plugins"
    plugins.mkdir(parents=True, exist_ok=True)
    dll = plugins / "latency_slider.dll"
    extension = ROOT / "extensions/net_delay"
    sources = [extension / name for name in ("core/delay_engine.cpp", "adapter/ssbu_1304.cpp",
                                              "adapter/preferences.cpp", "adapter/bridge.cpp")]
    native_compile(sources, dll, [ROOT / "runtime", ROOT / "fighter_sdk/include", ROOT / "mods/latency_slider",
                                 extension / "include"], ["bcrypt.lib", "advapi32.lib", "user32.lib"], shared=True)
    return dll


def source_fingerprint():
    paths = [ROOT / "mods/latency_slider/control.h", ROOT / "fighter_sdk/include/hook.h", Path(__file__),
             *[p for p in (ROOT / "extensions/net_delay").rglob("*") if p.is_file()],
             ROOT / "tools/console/ui/style.css", *[p for p in (ROOT / "tools/console/ui/fonts").glob("*") if p.is_file()]]
    return {p.relative_to(ROOT).as_posix(): digest(p) for p in sorted(paths)}


def build_extension(work, clang, elf):
    """Build the optional extension beside the normal host, independently of resource mods."""
    work = Path(work)
    verify_elf(elf)
    plugin = compile_plugin(clang, work / "plugins")
    frontend = compile_frontend(clang, work / "net_delay")
    artifacts = [plugin, frontend, frontend.parent / "WebView2-LICENSE.txt",
                 *[p for p in (frontend.parent / "ui").rglob("*") if p.is_file()]]
    stamp = dict(schema_version=1, source_sha256=source_fingerprint(),
                 artifacts_sha256={p.relative_to(work).as_posix(): digest(p) for p in artifacts})
    write_changed(work / "net_delay/build.json", (json.dumps(stamp, indent=2) + "\n").encode())
    return stamp


def webview_sdk():
    # Microsoft SDK matching the Console's installed WebView2 bindings. The
    # runtime is the same Evergreen WebView2 already used by Console.
    import urllib.request
    import zipfile
    version = "1.0.3650.58"
    expected = "911a472128c82ac8baa0c486c23342cc9dd6e7dc50d754e676726642ca065c60"
    folder = ROOT / "deps/webview2" / version
    package = folder / ("microsoft.web.webview2." + version + ".nupkg")
    folder.mkdir(parents=True, exist_ok=True)
    if not package.exists():
        temporary = package.with_suffix(".download")
        urllib.request.urlretrieve("https://api.nuget.org/v3-flatcontainer/microsoft.web.webview2/" + version + "/" + package.name, temporary)
        if digest(temporary) != expected:
            raise RuntimeError("WebView2 SDK checksum mismatch")
        temporary.replace(package)
    if digest(package) != expected:
        raise RuntimeError("WebView2 SDK checksum mismatch")
    with zipfile.ZipFile(package) as archive:
        for name in ("build/native/include/WebView2.h", "build/native/x64/WebView2LoaderStatic.lib", "LICENSE.txt"):
            write_changed(folder / name, archive.read(name))
    return folder


def compile_frontend(clang, output=None):
    output = Path(output) if output else OUT / "net_delay"
    output.mkdir(parents=True, exist_ok=True)
    sdk = webview_sdk()
    extension = ROOT / "extensions/net_delay"
    executable = output / "delay_panel.exe"
    native_compile([extension / "ui/win32_panel.cpp"], executable,
                   [extension / "include", sdk / "build/native/include"],
                   [sdk / "build/native/x64/WebView2LoaderStatic.lib", "user32.lib", "gdi32.lib", "ole32.lib",
                    "oleaut32.lib", "shlwapi.lib", "shell32.lib", "advapi32.lib", "version.lib"], windows=True)
    for path in (extension / "ui/web").glob("*"):
        if path.is_file():
            write_changed(output / "ui" / path.name, path.read_bytes())
    # Exact shared Console assets, not approximated colours/fonts/animations.
    for path in [ROOT / "tools/console/ui/style.css", *(ROOT / "tools/console/ui/fonts").glob("*")]:
        if path.is_file():
            relative = path.relative_to(ROOT / "tools/console/ui")
            write_changed(output / "ui/console" / relative, path.read_bytes())
    write_changed(output / "WebView2-LICENSE.txt", (sdk / "LICENSE.txt").read_bytes())
    return executable


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", type=Path, required=True, help="Existing repository with build/state.json")
    parser.add_argument("--plugin-only", action="store_true")
    args = parser.parse_args()
    base, state = load_base(args.base)
    if base == ROOT:
        raise ValueError("Use a separate existing build as --base; this experiment has its own output")
    elf = base / "build/binaries/13.0.4/main.elf"
    verify_elf(elf)
    tools = state["tools"]
    original_game = base / "build/work/ssbu_game_eh_o1.dll"
    originals = [elf, original_game, base / "build/work/ssbu_native.exe"]
    before = {str(p): digest(p) for p in originals}
    if args.plugin_only:
        plugin = compile_plugin(tools["clang"])
        print(f"Built plugin: {plugin}")
        return
    host = OUT / "host"
    host.mkdir(parents=True, exist_ok=True)
    sources = {}
    for path in (ROOT / "runtime").rglob("*"):
        if path.is_file():
            rel = path.relative_to(ROOT / "runtime")
            write_changed(host / rel, path.read_bytes())
            sources[rel.as_posix()] = digest(path)
    # Keep nested source layouts; never flatten r1src wrappers over root sources.
    for path in (ROOT / "tools/lifter").glob("*.py"):
        write_changed(host / path.name, path.read_bytes())
    for path in (base / "build/work").iterdir():
        if path.is_file() and (path.suffix in (".h", ".inc") or path.name in ("sdkdata.c", "sdklift_gen.c", "nro_registry.c")):
            if path.name not in sources:
                write_changed(host / path.name, path.read_bytes())
    for path in (ROOT / "fighter_sdk").rglob("*"):
        if path.is_file() and path.suffix in (".c", ".h", ".cpp", ".inc"):
            write_changed(host / "fighter_sdk" / path.relative_to(ROOT / "fighter_sdk"), path.read_bytes())
    generated_sdk = base / "build/work/fighter_sdk/src/fs_syms.inc"
    if generated_sdk.is_file():
        write_changed(host / "fighter_sdk/src/fs_syms.inc", generated_sdk.read_bytes())
    for name in ("ssbu_game_eh_o1.dll", "ssbu_game_eh_o1.lib", "ssbu_glslang.dll", "opus.dll"):
        path = base / "build/work" / name
        if path.is_file() and (not (host / name).exists() or digest(host / name) != digest(path)):
            shutil.copy2(path, host / name)
    # nnro.c resolves SSBU_NRO_BIN relative to the host executable. Stage copies
    # locally; passing an absolute cross-drive path would create an invalid path.
    modules = base / "build/work/nro_bin_eh_o2"
    module_hashes = {}
    for path in sorted(modules.glob("*.dll")):
        expected = digest(path)
        target = host / "nro_bin_eh_o2" / path.name
        target.parent.mkdir(exist_ok=True)
        if not target.exists() or digest(target) != expected:
            shutil.copy2(path, target)
        if digest(target) != expected:
            raise RuntimeError(f"Module changed while staging: {path.name}")
        module_hashes[str(target.relative_to(OUT))] = expected
    if not module_hashes:
        raise RuntimeError("The source build has no compiled game modules")
    env = clean_environment()
    env.update(PYTHONIOENCODING="utf-8", RECOMP_CLANG=tools["clang"], SDL3_ROOT=tools["sdl3"],
               VULKAN_HEADERS=tools["vulkan"], SSBU_NETWORK_DEPS=str(base / "deps"),
               RECOMP_GEN=str(base / "build/work/gen_eh_o2"), RECOMP_EXE="ssbu_latency_slider.exe")
    build_extension(host, tools["clang"], elf)
    plugin = host / "plugins/latency_slider.dll"
    frontend = host / "net_delay/delay_panel.exe"
    subprocess.run([sys.executable, str(host / "build.py"), "link", "--host-only", "--without-mods", "--with-latency-slider"],
                   cwd=host, env=env, check=True)
    after = {str(p): digest(p) for p in originals}
    if before != after:
        raise RuntimeError("Base build inputs changed during the build; investigate before launching")
    source_paths = [ROOT / "runtime/yuzu_net.c", ROOT / "runtime/yuzu_net.h", ROOT / "mods/latency_slider/control.h",
                    *[p for p in (ROOT / "extensions/net_delay").rglob("*") if p.is_file()]]
    result = dict(schema_version=2, base=str(base), base_sha256=before, target_sha256=TARGET_SHA256,
                  branch=subprocess.check_output(["git", "branch", "--show-current"], cwd=ROOT, text=True).strip(),
                  source_commit=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
                  yuzu_base=subprocess.check_output(["git", "merge-base", "HEAD", "yuzu"], cwd=ROOT, text=True).strip(),
                  resource_mods=False, host=str(host / "ssbu_latency_slider.exe"), plugin=str(plugin),
                  artifacts_sha256={str(p.relative_to(OUT)): digest(p) for p in
                                    (host / "ssbu_latency_slider.exe", host / "ssbu_game_eh_o1.dll", plugin, frontend,
                                     *[p for p in (host / "net_delay/ui").rglob("*") if p.is_file()])},
                  module_sha256=module_hashes,
                  changed_sources={str(p.relative_to(ROOT)): digest(p) for p in source_paths}, runtime_sha256=sources)
    write_changed(OUT / "build.json", (json.dumps(result, indent=2) + "\n").encode())
    print(f"PASS isolated build; base game/ELF/normal host SHA-256 unchanged\n{host / 'ssbu_latency_slider.exe'}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f"Cannot build delay experiment: {error}", file=sys.stderr)
        sys.exit(1)
