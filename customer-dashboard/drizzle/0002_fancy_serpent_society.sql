CREATE TABLE "cli_device_authorization" (
	"id" text PRIMARY KEY NOT NULL,
	"device_code_hash" text NOT NULL,
	"user_code_hash" text NOT NULL,
	"public_key" text NOT NULL,
	"thumbprint" text NOT NULL,
	"device_name" text NOT NULL,
	"platform" text NOT NULL,
	"cli_version" text NOT NULL,
	"status" text DEFAULT 'pending' NOT NULL,
	"attempts" integer DEFAULT 0 NOT NULL,
	"poll_interval" integer DEFAULT 5 NOT NULL,
	"last_polled_at" timestamp with time zone,
	"user_id" text,
	"license_id" text,
	"installation_id" text,
	"lease" jsonb,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"expires_at" timestamp with time zone NOT NULL,
	"consumed_at" timestamp with time zone,
	CONSTRAINT "cli_device_authorization_device_code_hash_unique" UNIQUE("device_code_hash")
);
--> statement-breakpoint
CREATE TABLE "cli_installation" (
	"id" text PRIMARY KEY NOT NULL,
	"user_id" text NOT NULL,
	"license_id" text NOT NULL,
	"public_key" text NOT NULL,
	"thumbprint" text NOT NULL,
	"device_name" text NOT NULL,
	"platform" text NOT NULL,
	"cli_version" text NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"last_authorized_at" timestamp with time zone DEFAULT now() NOT NULL,
	"revoked_at" timestamp with time zone
);
--> statement-breakpoint
ALTER TABLE "cli_device_authorization" ADD CONSTRAINT "cli_device_authorization_user_id_user_id_fk" FOREIGN KEY ("user_id") REFERENCES "public"."user"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "cli_device_authorization" ADD CONSTRAINT "cli_device_authorization_license_id_license_id_fk" FOREIGN KEY ("license_id") REFERENCES "public"."license"("id") ON DELETE set null ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "cli_installation" ADD CONSTRAINT "cli_installation_user_id_user_id_fk" FOREIGN KEY ("user_id") REFERENCES "public"."user"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "cli_installation" ADD CONSTRAINT "cli_installation_license_id_license_id_fk" FOREIGN KEY ("license_id") REFERENCES "public"."license"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
CREATE INDEX "cli_installation_user_active_idx" ON "cli_installation" USING btree ("user_id","revoked_at");--> statement-breakpoint
CREATE UNIQUE INDEX "cli_installation_user_thumbprint_unique" ON "cli_installation" USING btree ("user_id","thumbprint");