// SPDX-FileCopyrightText: 2024 Softbear, Inc.
// SPDX-License-Identifier: AGPL-3.0-or-later

use stylist::yew::styled_component;
use yew::prelude::*;
use web_sys::{HtmlInputElement, MouseEvent};
use wasm_bindgen::JsCast;
use crate::KiometGame;
use kodiak_client::{use_settings, use_browser_storages};
use js_sys::Function;
use wasm_bindgen::prelude::*;
use crate::ui::button::Button;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = window)]
    fn kiomet_set_server_address(server_url: &str) -> bool;
    
    #[wasm_bindgen(js_namespace = window)]
    fn kiomet_connect_to_server() -> bool;
}

// 添加一个函数来从localStorage获取服务器地址
fn get_saved_server_address() -> Option<String> {
    if let Some(window) = web_sys::window() {
        if let Ok(Some(storage)) = window.local_storage() {
            if let Ok(Some(address)) = storage.get_item("kiomet_server_address") {
                return Some(address);
            }
        }
    }
    None
}

#[styled_component(ServerAddressInput)]
pub fn server_address_input() -> Html {
    let input_ref = use_node_ref();
    let settings = use_settings::<KiometGame>();
    let browser_storages = use_browser_storages();
    // 默认不显示任何地址，即使localStorage中有保存的地址
    let server_address = use_state(String::default);
    let saved = use_state(|| false);
    
    let container_css = css!(
        r#"
        margin-top: 1.5rem;
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        width: 100%;
        "#
    );
    
    let input_container_css = css!(
        r#"
        display: flex;
        align-items: center;
        justify-content: center;
        width: 100%;
        margin-bottom: 0.5rem;
        "#
    );
    
    let input_css = css!(
        r#"
        background-color: rgba(30, 30, 30, 0.7);
        border: 1px solid rgba(255, 255, 255, 0.3);
        border-radius: 0.5rem;
        color: white;
        padding: 0.5rem;
        margin-right: 0.5rem;
        width: 70%;
        font-size: 0.9rem;
        outline: none;
        
        &:focus {
            border-color: rgba(255, 255, 255, 0.5);
            box-shadow: 0 0 5px rgba(255, 255, 255, 0.3);
        }
        "#
    );
    
    let saved_css = css!(
        r#"
        color: rgba(0, 255, 0, 0.7);
        font-size: 0.8rem;
        margin-left: 0.5rem;
        opacity: ${opacity};
        transition: opacity 0.5s;
        "#,
        opacity = if *saved { "1" } else { "0" }
    );
    
    let placeholder = "输入服务器WebSocket地址...";
    
    let onchange = {
        let server_address = server_address.clone();
        Callback::from(move |e: Event| {
            let target = e.target().unwrap();
            let input = target.dyn_ref::<HtmlInputElement>().unwrap();
            server_address.set(input.value());
        })
    };
    
    let onclick = {
        let server_address = server_address.clone();
        let saved = saved.clone();
        Callback::from(move |_: MouseEvent| {
            let address = (*server_address).clone();
            if !address.is_empty() {
                if kiomet_set_server_address(&address) {
                    saved.set(true);
                    // 3秒后隐藏保存提示
                    let saved_clone = saved.clone();
                    let closure = Closure::once_into_js(move || {
                        saved_clone.set(false);
                    });
                    web_sys::window()
                        .unwrap()
                        .set_timeout_with_callback_and_timeout_and_arguments_0(
                            closure.as_ref().unchecked_ref(),
                            3000,
                        )
                        .unwrap();
                }
            }
        })
    };
    
    let onclick_connect = Callback::from(move |_: MouseEvent| {
        kiomet_connect_to_server();
    });
    
    html! {
        <div class={container_css}>
            <div class={input_container_css}>
                <input
                    ref={input_ref}
                    type="text"
                    class={input_css}
                    placeholder={placeholder}
                    value={(*server_address).clone()}
                    {onchange}
                />
                <Button
                    onclick={onclick}
                    style="background: #006600; padding: 0.3rem 0.6rem;"
                >
                    {"✓"}
                </Button>
                <span class={saved_css}>{"已保存"}</span>
            </div>
            
            <Button
                onclick={onclick_connect}
                style="background: #000066; padding: 0.5rem 1rem; width: 70%; margin-top: 0.5rem;"
            >
                {"连接到服务器"}
            </Button>
        </div>
    }
} 