import { Button as BaseButton } from '@base-ui/react/button'
import { cva, type VariantProps } from 'class-variance-authority'
import { cn } from '../../lib/utils'

const buttonVariants = cva(
  'inline-flex shrink-0 items-center justify-center gap-2 whitespace-nowrap font-medium transition-colors focus-visible:outline-none disabled:pointer-events-none disabled:opacity-50',
  {
    variants: {
      variant: {
        primary: 'rounded-md border border-blue-500 bg-blue-600 text-white hover:bg-blue-500',
        secondary: 'rounded-md border border-slate-700 bg-slate-900 text-slate-100 hover:bg-slate-800',
        ghost: 'rounded-md text-slate-300 hover:bg-slate-800 hover:text-white',
      },
      size: {
        default: 'min-h-9 px-3 text-xs',
        sm: 'min-h-8 px-2.5 text-xs',
        icon: 'size-9',
      },
    },
    defaultVariants: { variant: 'primary', size: 'default' },
  },
)

type ButtonProps = React.ComponentProps<typeof BaseButton> & VariantProps<typeof buttonVariants>

export function Button({ className, variant, size, ...props }: ButtonProps) {
  return <BaseButton className={cn(buttonVariants({ variant, size }), className)} {...props} />
}
