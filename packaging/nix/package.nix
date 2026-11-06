{
  lib,
  stdenv,
  rustPlatform,
  pkg-config,
  cmake,
  copyDesktopItems,
  makeDesktopItem,
  ninja,
  runCommand,
  wrapGAppsHook3,
  cef-binary,
  gtk3,
  libGL,
  libopus,
  ffmpeg,
  soapysdr,
  vulkan-loader,
  soapyPlugins ? [ ],
  sdrplayApi ? null,
}:

let
  runtimeLibraries = [
    libGL
    vulkan-loader
  ]
  ++ lib.optional (sdrplayApi != null) sdrplayApi;

  # The Chromium build `cef-dll-sys` in Cargo.lock was generated for. Nix has no network in the
  # sandbox, so the archive is pinned here by the SHA-1 the CEF index publishes.
  cefVersion = "156.0.15";
  cefArchive = {
    x86_64-linux = {
      platform = "linux64";
      hash = "sha1-CSNUJyIuAmpPNDmXQt8nxdmKT4o=";
    };
    aarch64-linux = {
      platform = "linuxarm64";
      hash = "sha1-hWAtuOgTrYJLRa5UIYR2qj2R1WE=";
    };
  };
  cef = cef-binary.override {
    version = cefVersion;
    gitRevision = "cf671c0";
    chromiumVersion = "156.0.8078.80";
    srcHashes = lib.mapAttrs (_: archive: archive.hash) cefArchive;
  };
  archive = cefArchive.${stdenv.hostPlatform.system};
  # The layout `cef-dll-sys` reads from CEF_PATH: the runtime beside the SDK.
  cefDir = runCommand "sdrmm-cef-${cefVersion}" { } ''
    mkdir -p $out
    ln -s ${cef}/Release/* ${cef}/Resources/* $out/
    ln -s ${cef}/{CMakeLists.txt,cmake,include,libcef_dll,CREDITS.html} $out/
    cat > $out/archive.json <<EOF
    {"type":"minimal","name":"cef_binary_${cefVersion}+gcf671c0+chromium-156.0.8078.80_${archive.platform}_minimal.tar.bz2","sha1":""}
    EOF
  '';
in
rustPlatform.buildRustPackage (finalAttrs: {
  pname = "sdrmm-desktop";
  version = (builtins.fromTOML (builtins.readFile ../../Cargo.toml)).workspace.package.version;

  src = lib.cleanSourceWith {
    src = lib.cleanSource ../..;
    filter =
      path: type:
      !builtins.elem (baseNameOf path) [
        "target"
        "node_modules"
      ];
  };

  cargoLock = {
    lockFile = ../../Cargo.lock;
    allowBuiltinFetchGit = true;
  };

  nativeBuildInputs = [
    cmake
    ninja
    rustPlatform.bindgenHook
    copyDesktopItems
    pkg-config
    wrapGAppsHook3
  ];

  # cmake and ninja build CEF's wrapper from cargo, not as the package's own build system.
  dontUseCmakeConfigure = true;
  dontUseNinjaBuild = true;
  dontUseNinjaInstall = true;
  dontUseNinjaCheck = true;

  env.CEF_PATH = cefDir;

  buildInputs = [
    gtk3
    libopus
    (ffmpeg.override {
      withGPL = false;
      withVersion3 = false;
      withFdkAac = true;
    })
  ];

  # Nix fetches nothing without a pinned hash, so the web bundle comes prebuilt: the release
  # source tarball carries it, and a checkout needs `pnpm --dir web build` first.
  preBuild = ''
    test -f web/dist/index.html || {
      echo "web/dist is missing: build from the release source tarball, or run 'pnpm --dir web build' first" >&2
      exit 1
    }
  '';

  cargoBuildFlags = [
    "--package"
    "sdrmm-desktop"
  ];
  cargoTestFlags = finalAttrs.cargoBuildFlags;

  desktopItems = [
    (makeDesktopItem {
      name = "sdrmm-desktop";
      desktopName = "SDRmm";
      comment = "Software-defined radio receiver";
      exec = "sdrmm-desktop";
      icon = "dev.newspicel.sdrmm";
      categories = [
        "AudioVideo"
        "HamRadio"
      ];
    })
  ];

  # Chromium runs from beside the helper in lib/sdrmm-app, which the launcher's rpath names.
  postInstall = ''
    runtime="$out/lib/sdrmm-app"
    mkdir -p "$runtime"
    mv "$out/bin/sdrmm-desktop-helper" "$runtime/"
    for file in ${cef}/Release/* ${cef}/Resources/*; do
      case "$(basename "$file")" in
        chrome-sandbox | *.a) ;;
        *) ln -s "$file" "$runtime/" ;;
      esac
    done
    install -Dm644 apps/desktop/icons/128x128.png \
      "$out/share/icons/hicolor/128x128/apps/dev.newspicel.sdrmm.png"
    install -Dm644 apps/desktop/icons/128x128@2x.png \
      "$out/share/icons/hicolor/256x256/apps/dev.newspicel.sdrmm.png"
  '';

  # Nothing links SoapySDR: it is opened at runtime, and outside a Nix store there is no
  # default path to find it on. The wrapper names the store copy, and the plugins the user
  # selected stay separate packages it merely points at.
  preFixup = ''
    gappsWrapperArgs+=(
      --set-default SDRMM_SOAPY_LIBRARY "${soapysdr}/lib/libSoapySDR${stdenv.hostPlatform.extensions.sharedLibrary}"
      --prefix LD_LIBRARY_PATH : "${lib.makeLibraryPath runtimeLibraries}"
    )
  '' + lib.optionalString (soapyPlugins != [ ]) ''
    gappsWrapperArgs+=(
      --prefix SOAPY_SDR_PLUGIN_PATH : "${lib.makeSearchPath soapysdr.searchPath soapyPlugins}"
    )
  '';

  passthru = {
    inherit soapyPlugins sdrplayApi;
  };

  meta = {
    description = "Modular software-defined radio receiver desktop application";
    homepage = "https://sdrmm.com";
    license = lib.licenses.agpl3Plus;
    mainProgram = "sdrmm-desktop";
    platforms = lib.platforms.linux;
  };
})
