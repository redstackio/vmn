# Trusted Publishing

This project uses crates.io Trusted Publishing through GitHub Actions.

Use these values when configuring the trusted publisher on crates.io:

```text
Repository owner: redstackio
Repository name: vmn
Workflow filename: publish.yml
Environment name: release
```

The workflow file is:

```text
.github/workflows/publish.yml
```

Publishing is triggered by pushing a version tag:

```zsh
git tag v0.1.1
git push origin v0.1.1
```

The workflow exchanges GitHub's OIDC identity for a short-lived crates.io token using `rust-lang/crates-io-auth-action@v1`, then runs:

```zsh
cargo publish --locked
```
