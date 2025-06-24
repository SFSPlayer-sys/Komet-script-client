# Kiomet.com

[![Build](https://github.com/SoftbearStudios/kiomet/actions/workflows/build.yml/badge.svg)](https://github.com/SoftbearStudios/kiomet/actions/workflows/build.yml)
<a href='https://discord.gg/YMheuFQWTX'>
  <img src='https://img.shields.io/badge/Kiomet.com-%23announcements-blue.svg' alt='Kiomet.com Discord' />
</a>

![Logo](/assets/branding/512x340.jpg)

# English | [中文](#chinese)

## Introduction

[Kiomet.com](https://kiomet.com) is an online multiplayer real-time strategy game. Command your forces wisely and prepare for intense battles!

## Build Instructions

1. Install `rustup` ([see instructions here](https://rustup.rs/))
2. Install `gmake` and `gcc` if they are not already installed.
3. Install `trunk` (`cargo install --locked trunk --version 0.17.5`)
4. Run `download_makefiles.sh`
5. Install Rust Nightly and the WebAssembly target

```console
make rustup
```

6. Build client

```console
cd client
make release
```

7. Build and run server

```console
cd server
make run_release
```

8. Navigate to `https://localhost:8443/` and play!

## Custom Server

This modified version includes features for connecting to custom servers:
- Server address input in the login screen
- Custom WebSocket connection support
- Full game state access through JavaScript API

## JavaScript API Usage

The following JavaScript functions are available for interacting with the game:

### Get Game Information
```javascript
// Get complete game state
const gameState = window.wasm_bindgen.kiomet_get_full_state();

// Get all towers
const towers = window.wasm_bindgen.kiomet_get_towers();

// Get specific tower details
const towerDetails = window.wasm_bindgen.kiomet_get_tower_detail(towerId);

// Get all forces
const forces = window.wasm_bindgen.kiomet_get_forces();

// Get all players
const players = window.wasm_bindgen.kiomet_get_players();

// Get game state information
const stateInfo = window.wasm_bindgen.kiomet_get_game_state();

// Get towers in a specific area
const areaTowers = window.wasm_bindgen.kiomet_get_area_towers(x1, y1, x2, y2);
```

### Game Control
```javascript
// Execute game command
window.wasm_bindgen.kiomet_do_action({
  type: "pan_camera",
  x: 100,
  y: 100,
  zoom: 10
});

// Select tower
window.wasm_bindgen.kiomet_do_action({
  type: "select_tower",
  tower_id: 12345
});

// Deselect tower
window.wasm_bindgen.kiomet_do_action({
  type: "deselect_tower"
});
```

### Server Connection
```javascript
// Set custom server address
window.wasm_bindgen.kiomet_set_server_address("wss://example.com/ws");

// Connect to custom server
window.wasm_bindgen.kiomet_connect_to_server();
```

## Official Server Notice

To avoid potential visibility-cheating, you are prohibited from using the open-source
client to play on official Kiomet server(s).

## Trademark

Kiomet is a trademark of Softbear, Inc.

---

<a name="chinese"></a>
# 中文 | [English](#english)

## 简介

[Kiomet.com](https://kiomet.com) 是一款在线多人实时策略游戏。指挥您的部队，准备迎接激烈的战斗！

## 构建说明

1. 安装 `rustup`（[查看安装说明](https://rustup.rs/)）
2. 如果尚未安装，请安装 `gmake` 和 `gcc`
3. 安装 `trunk`，使用以下命令：
   ```
   cargo install --locked trunk --version 0.17.5
   ```
4. 运行 `download_makefiles.sh`
5. 安装 Rust Nightly 和 WebAssembly 目标：
   ```
   make rustup
   ```
6. 构建客户端：
   ```
   cd client
   make release
   ```
8. 导航到 `https://localhost:8443/` 开始游戏！

## 自定义服务器

此修改版本包含连接自定义服务器的功能：
- 登录界面中的服务器地址输入框
- 自定义WebSocket连接支持
- 通过JavaScript API完全访问游戏状态

## JavaScript API 使用方法

以下JavaScript函数可用于与游戏交互：

### 获取游戏信息
```javascript
// 获取完整游戏状态
const gameState = window.wasm_bindgen.kiomet_get_full_state();

// 获取所有塔
const towers = window.wasm_bindgen.kiomet_get_towers();

// 获取特定塔的详细信息
const towerDetails = window.wasm_bindgen.kiomet_get_tower_detail(towerId);

// 获取所有部队
const forces = window.wasm_bindgen.kiomet_get_forces();

// 获取所有玩家
const players = window.wasm_bindgen.kiomet_get_players();

// 获取游戏状态信息
const stateInfo = window.wasm_bindgen.kiomet_get_game_state();

// 获取特定区域的塔
const areaTowers = window.wasm_bindgen.kiomet_get_area_towers(x1, y1, x2, y2);
```

### 游戏控制
```javascript
// 执行游戏命令
window.wasm_bindgen.kiomet_do_action({
  type: "pan_camera",
  x: 100,
  y: 100,
  zoom: 10
});

// 选择塔
window.wasm_bindgen.kiomet_do_action({
  type: "select_tower",
  tower_id: 12345
});

// 取消选择塔
window.wasm_bindgen.kiomet_do_action({
  type: "deselect_tower"
});
```

### 服务器连接
```javascript
// 设置自定义服务器地址
window.wasm_bindgen.kiomet_set_server_address("wss://example.com/ws");

// 连接到自定义服务器
window.wasm_bindgen.kiomet_connect_to_server();
```

## 官方服务器声明

为避免可能的可见性作弊，禁止使用开源客户端连接官方Kiomet服务器。

## 商标

Kiomet 是 Softbear, Inc. 的商标。
