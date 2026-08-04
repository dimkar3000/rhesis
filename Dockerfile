FROM ghcr.io/flathub-infra/flatpak-github-actions:kde-6.10

RUN flatpak install --noninteractive -y \
    org.freedesktop.Sdk.Extension.rust-stable//24.08 \
    org.freedesktop.Sdk.Extension.openjdk17//24.08 \
    runtime/org.kde.Platform.Locale//6.11 \
    runtime/org.freedesktop.Sdk.Extension.openjdk17/x86_64/25.08 \
    runtime/org.freedesktop.Sdk.Extension.rust-stable/x86_64/25.08 \
    org.kde.Sdk//6.11 \
    org.kde.Platform//6.11 && \
    flatpak update --noninteractive -y

ENV SDK=/var/lib/flatpak/runtime/org.kde.Sdk/x86_64/6.11/active/files
ENV RUST_SDK=/var/lib/flatpak/runtime/org.freedesktop.Sdk.Extension.rust-stable/x86_64/24.08/active/files
ENV JAVA_HOME=/var/lib/flatpak/runtime/org.freedesktop.Sdk.Extension.openjdk17/x86_64/24.08/active/files/jvm/openjdk-17
ENV CMAKE_PREFIX_PATH=$SDK/lib/x86_64-linux-gnu/cmake:$SDK/share/ECM/cmake
ENV PATH=$SDK/bin:$RUST_SDK/bin:$JAVA_HOME/bin:$PATH

RUN echo "$SDK/lib/x86_64-linux-gnu" > /etc/ld.so.conf.d/sdk-qt.conf && ldconfig

COPY requirements.txt /tmp/requirements.txt
RUN pip3 install --no-cache-dir -r /tmp/requirements.txt flatpak-cargo-generator && \
    rm /tmp/requirements.txt

RUN wget -q -O /tmp/mold.tar.gz \
        "https://github.com/rui314/mold/releases/download/v2.36.0/mold-2.36.0-x86_64-linux.tar.gz" && \
    tar -xzf /tmp/mold.tar.gz -C /usr --strip-components=1 && \
    rm /tmp/mold.tar.gz

RUN mkdir -p /opt/appimage-tools && \
    wget -q -O /opt/appimage-tools/linuxdeploy-x86_64.AppImage \
        "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage" && \
    wget -q -O /opt/appimage-tools/linuxdeploy-plugin-qt-x86_64.AppImage \
        "https://github.com/linuxdeploy/linuxdeploy-plugin-qt/releases/download/continuous/linuxdeploy-plugin-qt-x86_64.AppImage" && \
    wget -q -O /opt/appimage-tools/appimagetool-x86_64.AppImage \
        "https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-x86_64.AppImage" && \
    chmod +x /opt/appimage-tools/linuxdeploy-x86_64.AppImage \
             /opt/appimage-tools/linuxdeploy-plugin-qt-x86_64.AppImage \
             /opt/appimage-tools/appimagetool-x86_64.AppImage
