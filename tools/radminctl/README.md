# radminctl for Linux (Pake isolated development branch)

Standalone Rust command-line frontend for the third-party
[baptisterajaut/radmin-vpn-linux](https://github.com/baptisterajaut/radmin-vpn-linux)
Wine/TAP compatibility backend. This folder is independent of Pake/Tauri.
The Radmin VPN protocol is proprietary and **is not reimplemented**.
No proprietary Windows binaries or the GPL backend are distributed here.

**Version 0.3.2**: CI-integrated standalone port of the earlier experimental radminctl.
This first CI branch uses a compact Rust implementation of process control,
LXC preflight and diagnostic export. It is not a verified native Radmin VPN implementation.
The earlier v0.3.1 source archive remains a separate prototype.

## GitHub Actions

Workflow: [.github/workflows/radminctl-linux-build.yml](../../.github/workflows/radminctl-linux-build.yml)

It runs on pushes to the isolated \`build/radminctl-linux-20261030\` branch
touching this folder or that workflow. It does not run Pake's existing release
workflows. Artifact \`radminctl-linux-amd64\` contains the GNU and static MUSL
tarballs, Debian package, and SHA256 hashes; \`radminctl-build-diagnostics\`
contains test/build/package logs. No release is published to Pake's release page.

## Install into Proxmox LXC (Debian, x86_64)

In a dedicated, preferably unprivileged LXC, give access to /dev/net/tun.
On the **Proxmox host** (replace 120 with CTID):

\`\`\`sh
modprobe tun
pct stop 120
cat >> /etc/pve/lxc/120.conf <<'EOF'
lxc.cgroup2.devices.allow: c 10:200 rwm
lxc.mount.entry: /dev/net/tun dev/net/tun none bind,create=file
EOF
pct start 120
\`\`\`

**Inside the container:**

\`\`\`sh
apt update
apt install -y iproute2 iputils-ping curl git bash sudo \
    gcc make mingw-w64 libgtk-4-dev upx-ucl python3
\`\`\`

The upstream backend requires Wine 11 or newer (verify with \`wine --version\`).
Use upstream instructions for installing that version.
Build the Wine/TAP backend separately:

\`\`\`sh
git clone https://github.com/baptisterajaut/radmin-vpn-linux.git /opt/radmin-vpn-linux
cd /opt/radmin-vpn-linux && make
\`\`\`

Download the successful GitHub Actions artifact from the **Actions** tab,
extract the .deb and install it:

\`\`\`sh
dpkg -i radminctl_0.3.2_amd64.deb
radminctl configure /opt/radmin-vpn-linux
radminctl selftest
radminctl lxc-check --tap-test
radminctl doctor
\`\`\`

For network join, the original Windows GUI is still required.
Use \`radminctl gui\` with a working X display, or in a test LXC:

\`\`\`sh
apt install -y xvfb x11vnc novnc websockify
read -rsp 'VNC password (8 ASCII chars): ' RADMINCTL_VNC_PASSWORD
export RADMINCTL_VNC_PASSWORD
radminctl web-ui
\`\`\`

The community backend may listen on **0.0.0.0:5900** even when
the noVNC web endpoint binds to localhost. Block TCP 5900 on the
container/Proxmox firewall. Use SSH tunnelling from your workstation:
\`ssh -L 6080:127.0.0.1:6080 root@CONTAINER_IP\`, then open
\`http://127.0.0.1:6080/vnc.html\`. The VNC password is passed as a
process argument by the community backend; use this only in an isolated
test container.

After joining the network, stop the GUI and run \`radminctl up\`
to start without GUI. The backend retains network state in its Wine prefix.
Do not delete that prefix or start GUI/headless services simultaneously.

## CLI

\`\`\`sh
radminctl configure /opt/radmin-vpn-linux
radminctl version
radminctl selftest
radminctl lxc-check --tap-test
radminctl lxc-check --json
radminctl doctor
radminctl diagnose --json
radminctl diagnose bundle ./report
radminctl diagnose bundle ./report-with-logs --include-logs
radminctl status --json
radminctl up
radminctl down
radminctl logs --source service --lines 50
radminctl neighbors
radminctl ping 26.1.2.3
\`\`\`

Diagnostic bundles default to metadata-only JSON with private 0700
directory / 0600 file permissions. \`--include-logs\` copies raw vendor
logs, which may contain sensitive information. Inspect before sharing.

## Build from source

\`\`\`sh
cd tools/radminctl
cargo test --offline
cargo build --offline --release
\`\`\`

Dependencies: Rust std only; Linux-only process/TAP checks.
Use \`ci/package-linux.sh\` after compiling GNU and MUSL targets.

## Limitations

- No native Radmin VPN handshake, registration, join/leave or peer enumeration.
- Live peer connections, Wine compatibility and LXC networking still require
  testing on a real Proxmox host.
- Diagnostics contain no automatic remediation of host security settings.
- This repository subtree must not be merged into Pake main without its
  maintainers' explicit decision; it is an isolated build workspace.
