FROM ghcr.io/flathub-infra/flatpak-github-actions:kde-6.10

RUN flatpak install --noninteractive -y \
    org.freedesktop.Sdk.Extension.rust-stable//24.08 \
    org.freedesktop.Sdk.Extension.openjdk17//24.08

COPY requirements.txt /tmp/requirements.txt
RUN pip3 install --no-cache-dir -r /tmp/requirements.txt flatpak-cargo-generator

RUN flatpak update --noninteractive -y
