# Terraform

Cloud infrastructure (managed Kubernetes, PostgreSQL, Redis, networking, DNS,
TLS, and the TURN relay) is provisioned with Terraform in **Phase 10**.

Planned modules: `network`, `kubernetes`, `database`, `cache`, `dns`, `turn`.
State is stored in a remote backend; environments (`staging`, `production`) are
separated via workspaces or directories.
