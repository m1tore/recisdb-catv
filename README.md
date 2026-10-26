# recisdb-catv

Rust で書かれたテレビチューナーリーダー / ARIB STD-B25 デコーダーである [recisdb](https://github.com/kazuki0824/recisdb-rs) をベースに、日本国内のケーブルテレビ（ISDB-C / CATV）および TBS6281SE v2 の PTX 互換ドライバ向けに拡張したフォーク版です。

---

## 本家（recisdb）との違い

本家 `recisdb` の基本機能（ISDB-T / ISDB-S の受信や B25 デコードなど）をそのまま継承しつつ、以下の機能を追加・拡張しています。

1. **JCTEA / ISDB-C（ケーブルテレビ）の完全対応**
   * CATV 帯（C13〜C63）および UHF 帯 ISDB-C（13〜62）の選局パラメータに対応。
   * 64QAM / 256QAM 自動追従スルー選局に対応し、CATV トランスモジュレーション波（BS / 専門チャンネル等）を受信可能。
2. **C-NIT ベースの高速チャンネルスキャン機能（`scan` サブコマンド）**
   * CATV 帯域を自動走査し、C-NIT（Cable NIT）および SDT / TSMF 情報を自動解析。
   * Mirakurun 向けの `channels.yml` を一発で自動生成可能。
   * CATV 専門チャンネルはデフォルトで `type: CS` として出力（`--catv` オプションで `type: CATV` に切り替え可能）。
3. **TSMF（複数TS多重化方式）のスロット分離**
   * 同一周波数に複数番組がスロット多重されている TSMF 搬送波から、指定スロットの TS のみを分離抽出可能。
4. **PTX 互換キャラクタデバイス向けチューニング**
   * TBS6281SE v2（`/dev/tbs6281video*`）の Linux PTX 互換ドライバ（[m1tore/TBS6281SEv2_drv](https://github.com/m1tore/TBS6281SEv2_drv)）との併用を推奨。

---

## インストール（ビルド）手順

Rust の開発環境が必要です。未インストールの場合は [Rustup](https://www.rust-lang.org/ja/tools/install) をインストールしてください。

### 1. 依存ツールのインストール（Ubuntu / Debian 例）

```bash
sudo apt update
sudo apt install -y build-essential cmake libclang-dev libpcsclite-dev libudev-dev pkg-config
```

### 2. リポジトリの取得とビルド

```bash
git clone --recursive https://github.com/<ユーザー名>/recisdb-catv.git
cd recisdb-catv

# リリースビルド
cargo build --release

# 実行バイナリを配置
sudo cp target/release/recisdb-catv /usr/local/bin/
```

---

## 使い方（Usage）

`recisdb-catv` は以下のサブコマンドに対応しています。

* `tune`: チャンネルを選局し、TS データを録画または標準出力にストリーミングします。
* `scan`: CATV 帯域を走査し、Mirakurun 用の `channels.yml` を自動生成します。
* `checksignal`: 信号の受信状態や CNR（信号品質）を確認します。
* `decode`: ARIB STD-B25 に基づき、スクランブルされた TS ファイルをデコードします。

### コマンド構文

```bash
# 録画・ストリーミング
recisdb-catv tune [OPTIONS] -i <デバイス> -c <チャンネル> [OUTPUT]

# チャンネルスキャン
recisdb-catv scan -i <デバイス> -o <出力YAMLファイル>

# 信号レベル確認
recisdb-catv checksignal -i <デバイス> -c <チャンネル>

# 単体デコード
recisdb-catv decode -i <入力ファイル> <出力ファイル>
```

---

## チャンネル指定フォーマット（-c / --channel）

### 1. ケーブルテレビ / CATV（ISDB-C / トランスモジュレーション）
* `-c C50` : CATV C50ch（NHK BS トランスモジュレーション波など）
* `-c C53` : CATV C53ch（BS日テレなど）
* `-c C28` : CATV C28ch（コミュニティチャンネル・自主放送など）
* `-c C30_1` または `-c C30:1` : CATV C30ch の TSMF スロット 1 のみを分離選局

### 2. 地上デジタル（ISDB-T）
* `-c 27` または `-c T27` : 地上波 27ch

### 3. BS / CS（衛星直受け時）
* `-c BS01_0` : BS01 の相対 TS 番号 0
* `-c CS02` : CS ND02

> [!TIP]
> Mirakurun や録画コマンドでは、放送種別を明示する `-t` / `--type` オプション（`GR`, `BS`, `CATV`, `CS`）を併用できます。
> 例: `recisdb-catv tune -i /dev/tbs6281video0 -c C50 -t BS recorded.ts`

---

## コマンド実行例（Examples）

TBS6281SE v2（`/dev/tbs6281video*`）を使用する場合の代表的なコマンド例です。

### 1. チャンネルスキャン（Mirakurun 用 channels.yml 自動生成）
```bash
# CATV 全帯域をスキャン（専門チャンネルを type: CS として出力・推奨）
recisdb-catv scan -i /dev/tbs6281video0 -o /etc/mirakurun/channels.yml

# CATV 専門チャンネルを type: CATV として出力する場合
recisdb-catv scan -i /dev/tbs6281video0 --catv -o /etc/mirakurun/channels.yml
```

### 2. CATV（BS トランスモジュレーション）の録画
```bash
# NHK BS（C50ch）を B25 デコードしながら 10 秒間録画
recisdb-catv tune -i /dev/tbs6281video0 -c C50 -t BS --time 10 nhk_bs.ts

# BS日テレ（C53ch）をデコードなし（生TS）で録画
recisdb-catv tune -i /dev/tbs6281video0 -c C53 -t BS --no-decode --time 10 bs_ntv.ts
```

### 3. 地上波の信号確認・録画
```bash
# 地上波 27ch の信号レベル（CNR）を確認
recisdb-catv checksignal -i /dev/tbs6281video0 -c 27

# 地上波 27ch を録画
recisdb-catv tune -i /dev/tbs6281video0 -c 27 -t GR --time 10 gr27.ts
```

### 5. TS ファイルの単体 B25 デコード
```bash
# スクランブルされた TS ファイルをデコード
recisdb-catv decode -i scrambled.ts descrambled.ts
```

---

## 動作確認済み環境

* **OS**: Linux（Kernel 6.x / 7.x）
* **対応ハードウェア**: TurboSight TBS6281SE v2
* **対応ドライバ**: [m1tore/TBS6281SEv2_drv](https://github.com/m1tore/TBS6281SEv2_drv)（PTX 互換キャラクタデバイスドライバ）

※Windows および macOS での動作検証は行っておりません。

---

## ライセンス（License）

本ソフトウェアは [GPL v3](LICENSE) の下で公開されています。
