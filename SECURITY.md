# Security policy

## Supported versions

Security fixes are provided for the newest published version of Lantern.

## Reporting a vulnerability

Please do not open a public issue for a suspected vulnerability. Use GitHub's **Security → Report a vulnerability** flow to create a private security advisory for this repository.

Include the affected version, platform, reproduction steps, impact, and any suggested mitigation. Remove passwords, private keys, hostnames, MAC addresses, and other sensitive network details from the report. You should receive an acknowledgement within seven days.

Lantern intentionally delegates SSH cryptography, host-key verification, agents, and SSH configuration to the operating system's OpenSSH client. Reports about OpenSSH itself should be sent to the relevant upstream project or operating-system vendor.

