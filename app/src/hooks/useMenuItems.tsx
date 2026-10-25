import {
  Settings,
  PowerIcon,
  TerminalIcon,
  PlaneIcon,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";

/**
 * The dashboard window's navigation. Only the Wright surfaces remain: the
 * Pluely chat verticals (chats, system prompts, responses, screenshot, audio,
 * dev space, the old dashboard) were removed with the chat product, along
 * with the license-gated support link and upstream's promotional footer.
 */
export const useMenuItems = () => {
  const menu: {
    icon: React.ElementType;
    label: string;
    href: string;
    count?: number;
  }[] = [
    { icon: PlaneIcon, label: "Wright", href: "/wright" },
    { icon: Settings, label: "Settings", href: "/settings" },
    { icon: TerminalIcon, label: "Logs", href: "/console" },
  ];

  const footerItems = [
    {
      icon: PowerIcon,
      label: "Quit Wright",
      action: async () => {
        await invoke("exit_app");
      },
    },
  ];

  const footerLinks: {
    title: string;
    icon: React.ElementType;
    link: string;
  }[] = [];

  return {
    menu,
    footerItems,
    footerLinks,
  };
};
