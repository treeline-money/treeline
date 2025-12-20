import type { Plugin, PluginContext, PluginSDK } from "@treeline-money/plugin-sdk";
import HelloWorldView from "./HelloWorldView.svelte";
import { mount, unmount } from "svelte";

export const plugin: Plugin = {
  manifest: {
    id: "hello-world",
    name: "Hello World",
    version: "0.1.0",
    description: "An example plugin demonstrating the Treeline plugin SDK",
    author: "Your Name",
    // Declare tables this plugin needs write access to.
    // Community plugins can only write to sys_plugin_{id}_* tables.
    permissions: {
      tables: {
        write: ["sys_plugin_hello_world"],
      },
    },
  },

  activate(context: PluginContext) {
    console.log("Hello World plugin activated!");

    // Register the view with a mount function
    // Use Lucide icon names like "target", "shield", "repeat", etc.
    context.registerView({
      id: "hello-world-view",
      name: "Hello World",
      icon: "zap",
      mount: (target: HTMLElement, props: { sdk: PluginSDK }) => {
        // Mount the Svelte component using our bundled Svelte runtime
        const instance = mount(HelloWorldView, {
          target,
          props,
        });

        // Return cleanup function
        return () => {
          unmount(instance);
        };
      },
    });

    // Add sidebar item
    context.registerSidebarItem({
      sectionId: "main",
      id: "hello-world",
      label: "Hello World",
      icon: "zap",  // Lucide icon name (or emoji like "👋")
      viewId: "hello-world-view",
    });

    // Register a command (optional)
    context.registerCommand({
      id: "hello-world.greet",
      name: "Say Hello",
      description: "Display a greeting message",
      execute: () => {
        console.log("👋 Hello from the Hello World plugin!");
      },
    });

    console.log("✓ Hello World plugin registered");
  },

  deactivate() {
    console.log("Hello World plugin deactivated");
  },
};
