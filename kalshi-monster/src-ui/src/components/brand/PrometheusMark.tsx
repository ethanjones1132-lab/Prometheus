import emblem from '../../assets/prometheus-emblem.png';
import medallion from '../../assets/prometheus-medallion.png';

/**
 * The Prometheus emblem — the all-seeing eye medallion wreathed in
 * black wings and gold arcs. `variant="emblem"` renders the full
 * artwork tile (brand header, hero); `variant="medallion"` renders the
 * circular eye cutout (avatars, compact marks).
 */
export function PrometheusMark({
  variant = 'medallion',
  className,
  alt = 'Prometheus emblem',
}: {
  variant?: 'emblem' | 'medallion';
  className?: string;
  alt?: string;
}) {
  const src = variant === 'emblem' ? emblem : medallion;
  return <img className={className} src={src} alt={alt} draggable={false} />;
}

export { emblem as prometheusEmblem, medallion as prometheusMedallion };
