FROM ghcr.io/flathub-infra/flatpak-github-actions:kde-6.10

RUN flatpak install --noninteractive -y \
    org.freedesktop.Sdk.Extension.rust-stable//24.08 \
    org.freedesktop.Sdk.Extension.openjdk17//24.08

COPY requirements.txt /tmp/requirements.txt
RUN pip3 install --no-cache-dir -r /tmp/requirements.txt flatpak-cargo-generator

RUN flatpak update --noninteractive -y

# Install mold linker (static binary, used via .cargo/config.toml's -fuse-ld=mold)
RUN wget -q -O /tmp/mold.tar.gz \
        "https://github.com/rui314/mold/releases/download/v2.36.0/mold-2.36.0-x86_64-linux.tar.gz" && \
    tar -xzf /tmp/mold.tar.gz -C /usr --strip-components=1 && \
    rm /tmp/mold.tar.gz

# Install AppImage tools (linuxdeploy, linuxdeploy-plugin-qt, appimagetool)
RUN mkdir -p /opt/appimage-tools && \
    wget -q -O /opt/appimage-tools/linuxdeploy-x86_64.AppImage \
        "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage" && \
    chmod +x /opt/appimage-tools/linuxdeploy-x86_64.AppImage && \
    wget -q -O /opt/appimage-tools/linuxdeploy-plugin-qt-x86_64.AppImage \
        "https://github.com/linuxdeploy/linuxdeploy-plugin-qt/releases/download/continuous/linuxdeploy-plugin-qt-x86_64.AppImage" && \
    chmod +x /opt/appimage-tools/linuxdeploy-plugin-qt-x86_64.AppImage && \
    wget -q -O /opt/appimage-tools/appimagetool-x86_64.AppImage \
        "https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-x86_64.AppImage" && \
    chmod +x /opt/appimage-tools/appimagetool-x86_64.AppImage
