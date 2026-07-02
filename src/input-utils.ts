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

type InputLikeEvent = {
  nativeEvent: Event;
};

export function isInputComposing(event: InputLikeEvent) {
  return Boolean((event.nativeEvent as Event & { isComposing?: boolean }).isComposing);
}
