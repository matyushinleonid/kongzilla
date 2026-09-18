<p align="center">
  <img src="web/public/logo.png" width="128" height="128" alt="">
</p>

<h1 align="center">Kongzilla</h1>

<p align="center">Hold'em range and board analysis, in the browser.</p>

<p align="center">
  <a href="https://kongzilla.leonid.sh"><img alt="Live" src="https://img.shields.io/website?url=https%3A%2F%2Fkongzilla.leonid.sh&up_message=live&down_message=down&label=kongzilla.leonid.sh&style=flat-square"></a>
  <a href="https://github.com/matyushinleonid/kongzilla/actions/workflows/ci.yml"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/matyushinleonid/kongzilla/ci.yml?branch=main&style=flat-square&label=ci"></a>
  <img alt="Engine" src="https://img.shields.io/badge/engine-Rust%20%E2%86%92%20WebAssembly-dea584?style=flat-square">
  <a href="https://github.com/matyushinleonid/kongzilla/stargazers"><img alt="Stars" src="https://img.shields.io/github/stars/matyushinleonid/kongzilla?style=flat-square&logo=github&label=star"></a>
  <a href="https://argocd.leonid.sh/applications/argocd/kongzilla"><img alt="Deployment" src="https://argocd.leonid.sh/api/badge?name=kongzilla"></a>
  <a href="LICENSE"><img alt="Licence" src="https://img.shields.io/github/license/matyushinleonid/kongzilla?style=flat-square"></a>
</p>

<p align="center"><b><a href="https://kongzilla.leonid.sh">kongzilla.leonid.sh</a></b></p>

---

Enter a range, enter a flop, and see what the range actually hits — then filter it
down to the hands that would keep going. An open clone of the analysis loop
[Flopzilla](https://www.flopzilla.com/) made standard. No install, no account, no
server.

```sh
make run      # http://localhost:5173
make check    # everything CI runs
```

Kongzilla is an independent project, not affiliated with Flopzilla or its authors.
MIT licensed.
