{
  lib,
  stdenv,
  rustPlatform,
  pkg-config,
  cmake,
  copyDesktopItems,
  makeDesktopItem,
  wrapGAppsHook3,
  cairo,
  gdk-pixbuf,
  glib,
  gtk3,
  libayatana-appindicator,
  libGL,
  libopus,
  ffmpeg,
  librsvg,
  libsoup_3,
  openssl,
  pango,
  soapysdr,
  vulkan-loader,
  webkitgtk_4_1,
  xdotool,
  soapyPlugins ? [ ],
  sdrplayApi ? null,
}:

let
  runtimeLibraries = [
    libGL
    vulkan-loader
  ]
  ++ lib.optional (sdrplayApi != null) sdrplayApi;
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
    rustPlatform.bindgenHook
    copyDesktopItems
    pkg-config
    wrapGAppsHook3
  ];

  buildInputs = [
    cairo
    gdk-pixbuf
    glib
    gtk3
    libayatana-appindicator
    libopus
    (ffmpeg.override {
      withGPL = false;
      withVersion3 = false;
      withFdkAac = true;
    })
    librsvg
    libsoup_3
    openssl
    pango
    webkitgtk_4_1
    xdotool
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

  postInstall = ''
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
