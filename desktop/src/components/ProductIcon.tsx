import { useEffect, useState } from "react";
import { api, ProductType } from "../api";
import { PaletteIcon, PlugIcon } from "./Icons";

const cache = new Map<string, string | null>();

/** The product's WordPress.org icon, or a gradient tile with the type's symbol. */
export function ProductIcon({ id, type, size = 26 }: { id: string; type: ProductType; size?: number }) {
  const [src, setSrc] = useState<string | null | undefined>(cache.get(id));
  useEffect(() => {
    if (cache.has(id)) return setSrc(cache.get(id));
    let live = true;
    api.productIcon(id).then((url) => {
      cache.set(id, url);
      if (live) setSrc(url);
    }, () => live && setSrc(null));
    return () => { live = false; };
  }, [id]);
  const style = { width: size, height: size, borderRadius: size * 0.25 };
  if (src) return <img className="product-icon" src={src} alt="" style={style} />;
  return (
    <span className={`product-glyph ${type}`} style={style} aria-hidden>
      {type === "plugin" ? <PlugIcon /> : <PaletteIcon />}
    </span>
  );
}
