# T-FLATHUB-001: Flathub に Kagi を登録する

- Status: todo
- Group: 配布 / パッケージング
- 仕様の正: https://docs.flathub.org/docs/for-app-authors/submission (外部プロセス), ADR-0047(配布 Phase 1)

## 背景(調査済み)

パッケージマネージャ配布計画の Linux 本命。`.deb`(PR #98、リリース資産に追加済み)は
`dpkg -i` 止まりで更新チャネルがない。Flathub は Linux GUI アプリ配布の事実上の標準で、
更新配信・サンドボックス・ストア掲載が付いてくる。素材は揃っている:
リリース資産(tar.gz / AppImage)、`assets/linux/kagi.desktop`、512px アイコン。

## スコープ

1. **App ID 決定**: `io.github.tomixrm.kagi`(GitHub Pages 不要の github プレフィックス形式)。
2. **本 repo 側に追加**(このチケットの PR 範囲):
   - `assets/linux/io.github.tomixrm.kagi.metainfo.xml`(AppStream メタデータ:
     名前・説明・スクリーンショット URL・ライセンス MIT・release タグ)。
     `appstreamcli validate` を CI か手元で通すこと。
   - `.desktop` / アイコンのファイル名を App ID に合わせるかは Flathub manifest 側の
     rename ディレクティブで吸収可(本 repo の改名は不要)。
3. **Flathub 側**(外部、人間の GitHub 操作が必要):
   - `flathub/flathub` に submission PR: manifest `io.github.tomixrm.kagi.yml` は
     リリースの Linux tar.gz を source に取り、bin/desktop/icon/metainfo を install。
     ビルド済みバイナリ取り込み(extra-data ではなく通常 archive source)で開始し、
     審査で from-source を求められたら cargo ビルド化を検討(gpui 依存が重いので注意)。
   - finish-args: `--share=ipc --socket=wayland --socket=fallback-x11 --device=dri
     --filesystem=home`(Git repo を開くため。縮められるなら縮める)。
4. リリースごとの更新は Flathub bot(external-data-checker)で自動 PR 化できる —
   x-checker-data を manifest に書く。

## 触ってよいファイル(本 repo)

- `assets/linux/`(metainfo 追加)、`docs/tickets/`、必要なら release.yml(metainfo の validate ステップ)。

## 完了条件

- [ ] metainfo.xml が `appstreamcli validate` を通る。
- [ ] Flathub submission PR が open され、ビルドが green。
- [ ] `flatpak install flathub io.github.tomixrm.kagi` で起動できる。

## リスク

- 審査対応(from-source 要求、finish-args の絞り込み指摘)に往復が発生し得る。
- git 実行を flatpak サンドボックス内から行う都合上 `--filesystem` の広さは要検討。

## やってはいけないこと

App ID の後変更(一度公開すると実質不可)/ 本 repo の .desktop/icon の安易な rename。
