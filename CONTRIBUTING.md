# contributing

## requirements

### mise

Install `mise-en-place`: https://mise.jdx.dev/ 

### linux
```sh
sudo apt install libasound2-dev pkg-config build-essential
```

## development

### validation
```sh
mise run doctor
```

### play
```sh
mise tui
```

### test
Local:
```sh
mise run test:local
```

Everything (local, E2E, etc.):
```sh
mise tests
```

### lint
```sh
mise lint
```

## guidelines

- [ ] [`RULES.md`](RULES.md) followed
- [ ] [`TERMS.md`](TERMS.md) consistently applied
- [ ] `mise bump {major|minor|patch}` versioned (as appropriate per [semver](https://semver.org))
- [ ] [`LICENSE`](LICENSE) honored


