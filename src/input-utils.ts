type KeyboardLikeEvent = {
  key: string;
  keyCode?: number;
  nativeEvent: {
    isComposing?: boolean;
  };
};

export function isImeComposing(event: KeyboardLikeEvent) {
  return Boolean(event.nativeEvent.isComposing) || event.key === "Process" || event.keyCode === 229;
}
