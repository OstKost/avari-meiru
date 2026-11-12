FROM --platform=linux/amd64 ubuntu:24.04 AS build
ARG VERSION=0.1.0
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends build-essential curl file pkg-config libssl-dev libgtk-3-dev libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev patchelf ca-certificates && rm -rf /var/lib/apt/lists/*
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.98.1 --profile minimal
ENV PATH=/root/.cargo/bin:$PATH
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/root/.cargo/registry --mount=type=cache,target=/root/.cargo/git cd desktop/src-tauri && cargo build --release --locked -p mieru-desktop -p mieru-helper
RUN mkdir -p /out/usr/bin /out/usr/lib/mieru /out/usr/share/polkit-1/actions /out/usr/share/applications /out/usr/share/icons/hicolor/128x128/apps /out/usr/share/doc/mieru
RUN install -m755 desktop/src-tauri/target/release/mieru-desktop /out/usr/bin/mieru
RUN install -m755 desktop/src-tauri/target/release/mieru-helper /out/usr/lib/mieru/mieru-helper
RUN install -m755 core/dist/sing-box-x86_64-unknown-linux-gnu /out/usr/lib/mieru/sing-box
RUN install -m644 desktop/src-tauri/linux/app.mieru.policy /out/usr/share/polkit-1/actions/app.mieru.policy
RUN install -m644 desktop/src-tauri/icons/128x128.png /out/usr/share/icons/hicolor/128x128/apps/mieru.png
RUN printf '[Desktop Entry]\nType=Application\nName=Mieru\nComment=Mieru desktop client\nExec=/usr/bin/mieru\nIcon=mieru\nTerminal=false\nCategories=Network;\n' > /out/usr/share/applications/mieru.desktop && chmod 644 /out/usr/share/applications/mieru.desktop
RUN cp core/dist/LICENSE /out/usr/share/doc/mieru/LICENSE-sing-box && cp core/dist/licenses/mieru-v3.38.0.LICENSE /out/usr/share/doc/mieru/LICENSE-mieru && chmod 644 /out/usr/share/doc/mieru/*
RUN printf '%s\n' "$VERSION" > /out/VERSION
FROM scratch AS export
COPY --from=build /out /out
