import type { ChatChannelRead } from "$lib/chat/contracts";
import { getPreferences } from "$lib/stores/preferences.svelte";
import {
  customOrderRanks,
  parseCustomOrders,
  parseSidebarSortOrderOverrides,
  parseStarredIds,
  resolveSidebarSortOrder,
  sortSidebarItems,
  withCustomSiblingOrder,
  withSidebarSortOrderOverride,
  withStarredId,
  type SidebarSortOrder,
} from "$lib/utils/sidebar-order";
import { configKeyChanged, getConfigKey, onConfigChangedElsewhere, setConfigKey } from "$lib/vault/config";

const STARRED_CHANNEL_IDS_CONFIG_KEY = "chatSidebar.starredChannelIds";
const SIDEBAR_ORDER_BY_PROJECT_CONFIG_KEY = "chatSidebar.orderByProject";
const CUSTOM_ORDER_BY_PROJECT_CONFIG_KEY = "chatSidebar.customOrderByProject";

/** Starred Chat channels and the Chat sidebar order, global with optional per-project overrides. */
export interface ChatSidebarOrderStore {
  /** Whether the channel is starred, which lists it first in its section. */
  isChannelStarred(channelId: string): boolean;
  setChannelStarred(channelId: string, starred: boolean): void;
  /** The project's own order, or `null` when it follows the global Chat order. */
  orderOverride(projectId: string | null): SidebarSortOrder | null;
  /** The order the project's sections and channels are listed in. */
  orderForProject(projectId: string | null): SidebarSortOrder;
  /** Set the project's own order, or follow the global order again with `null`. */
  setProjectOrder(projectId: string, order: SidebarSortOrder | null): void;
  /** Channel ids of the project in their custom order. */
  customOrder(projectId: string | null): readonly string[];
  /** Save the custom order of channels that trade places, such as one section's channels, in the project. */
  setCustomSiblingOrder(projectId: string, orderedChannelIds: readonly string[]): void;
  /** Sort a copy of one project's channels by its order, starred channels first. */
  sortChannels(projectId: string | null, channels: readonly ChatChannelRead[]): ChatChannelRead[];
}

function createChatSidebarOrder(): ChatSidebarOrderStore {
  const preferences = getPreferences();
  let starredChannelIds = $state<string[]>(parseStarredIds(getConfigKey<unknown>(STARRED_CHANNEL_IDS_CONFIG_KEY, undefined)));
  let orderOverrides = $state<Record<string, SidebarSortOrder>>(
    parseSidebarSortOrderOverrides(getConfigKey<unknown>(SIDEBAR_ORDER_BY_PROJECT_CONFIG_KEY, undefined)),
  );
  let customOrders = $state<Record<string, string[]>>(
    parseCustomOrders(getConfigKey<unknown>(CUSTOM_ORDER_BY_PROJECT_CONFIG_KEY, undefined)),
  );

  onConfigChangedElsewhere((keys) => {
    if (configKeyChanged(keys, STARRED_CHANNEL_IDS_CONFIG_KEY)) {
      starredChannelIds = parseStarredIds(getConfigKey<unknown>(STARRED_CHANNEL_IDS_CONFIG_KEY, undefined));
    }
    if (configKeyChanged(keys, SIDEBAR_ORDER_BY_PROJECT_CONFIG_KEY)) {
      orderOverrides = parseSidebarSortOrderOverrides(getConfigKey<unknown>(SIDEBAR_ORDER_BY_PROJECT_CONFIG_KEY, undefined));
    }
    if (configKeyChanged(keys, CUSTOM_ORDER_BY_PROJECT_CONFIG_KEY)) {
      customOrders = parseCustomOrders(getConfigKey<unknown>(CUSTOM_ORDER_BY_PROJECT_CONFIG_KEY, undefined));
    }
  });

  function orderOverride(projectId: string | null): SidebarSortOrder | null {
    return projectId ? orderOverrides[projectId] ?? null : null;
  }

  function orderForProject(projectId: string | null): SidebarSortOrder {
    return resolveSidebarSortOrder(preferences.chatSidebarOrder, orderOverride(projectId));
  }

  function customOrder(projectId: string | null): readonly string[] {
    return projectId ? customOrders[projectId] ?? [] : [];
  }

  function isChannelStarred(channelId: string): boolean {
    return starredChannelIds.includes(channelId);
  }

  return {
    isChannelStarred,
    setChannelStarred(channelId, starred) {
      if (starredChannelIds.includes(channelId) === starred) return;
      starredChannelIds = withStarredId(starredChannelIds, channelId, starred);
      setConfigKey(STARRED_CHANNEL_IDS_CONFIG_KEY, starredChannelIds.length > 0 ? [...starredChannelIds] : undefined);
    },
    orderOverride,
    orderForProject,
    setProjectOrder(projectId, order) {
      orderOverrides = withSidebarSortOrderOverride(orderOverrides, projectId, order);
      setConfigKey(SIDEBAR_ORDER_BY_PROJECT_CONFIG_KEY, Object.keys(orderOverrides).length > 0 ? { ...orderOverrides } : undefined);
    },
    customOrder,
    setCustomSiblingOrder(projectId, orderedChannelIds) {
      customOrders = { ...customOrders, [projectId]: withCustomSiblingOrder(customOrder(projectId), orderedChannelIds) };
      setConfigKey(CUSTOM_ORDER_BY_PROJECT_CONFIG_KEY, { ...customOrders });
    },
    sortChannels(projectId, channels) {
      const ranks = customOrderRanks(customOrder(projectId));
      return sortSidebarItems(
        channels,
        orderForProject(projectId),
        (channel) => ({
          id: channel.id,
          name: channel.name,
          createdAt: channel.createdAt,
          modifiedAt: channel.lastActivityAt,
          rank: ranks.get(channel.id),
        }),
        (channel) => isChannelStarred(channel.id),
      );
    },
  };
}

let store: ChatSidebarOrderStore | null = null;

/** Return the Chat sidebar order store, reading its saved state on first use after the vault config loads. */
export function getChatSidebarOrder(): ChatSidebarOrderStore {
  store ??= createChatSidebarOrder();
  return store;
}
