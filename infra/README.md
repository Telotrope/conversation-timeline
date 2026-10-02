# Deploying to AWS: first-time setup

Setting up an AWS account, the tools, a domain's certificate, and the first deployment of each
stack; and tearing it all down. Everything repeated (redeploying, testing, diagnosing) is in
[OPERATING.md](OPERATING.md). The design is in
[docs/plans/2026-10-02-deployment-operating-guide.md](../docs/plans/2026-10-02-deployment-operating-guide.md).

Terms used below:

- **Region**: the AWS data-center location everything is created in; here always `us-east-1`.
- **Stack**: the set of resources AWS creates from the template ([template.yaml](template.yaml)),
  created and deleted as one. There are two: `timeline-dev` (remote development) and
  `timeline-public`.
- **IAM**: AWS's users-and-permissions system.

## 1. Make the account safe (once)

1. Sign in as the **root user** (the email address that owns the account). Under your name →
   *Security credentials*, turn on multi-factor sign-in.
2. *Billing and Cost Management → Budgets → Create budget*: a monthly cost budget of about $10 that
   emails you. This is your warning against surprise charges.
3. Open **IAM Identity Center**, enable it, create a user for yourself, and give it the
   `AdministratorAccess` permission set on this account. Use that user from now on, not root.

## 2. Install the tools (once, on this machine)

- **AWS CLI v2** (the `aws` command): AWS's "Install or update the AWS CLI" page, Linux x86_64. To
  install without `sudo`, unzip it and run
  `./aws/install --install-dir ~/.local/aws-cli --bin-dir ~/.local/bin`.
- **SAM CLI** (the `sam` command): AWS's "Installing the AWS SAM CLI" page, Linux x86_64. Without
  `sudo`: `./sam-installation/install --install-dir ~/.local/sam --bin-dir ~/.local/bin`.
  (The development session that wrote this installed it the same way into a scratch folder and ran
  the template check with it.)
- **cargo-lambda and Zig**, for building: see [backend/README.md](../backend/README.md). On this
  machine both are already installed; Zig lives in `~/.local/opt/zig`.

Make sure `~/.local/bin` is on your `PATH`, then check: `aws --version` and `sam --version`.
(`scripts/deploy.sh` also looks in `~/.local/bin`, `~/.cargo/bin` and `~/.local/opt/zig` itself.)

## 3. Sign in from the command line

```
aws login --profile timeline --remote   # opens a browser sign-in; lasts under a day
export AWS_PROFILE=timeline AWS_REGION=us-east-1
aws sts get-caller-identity             # should print your 12-digit account number
```

`aws sso login` doesn't work on this machine. The scripts use the `timeline` profile by default
and tell you to run `aws login` again when the sign-in has expired.

## 4. A certificate for each domain (once per domain)

Each stack serves its page at its own domain (`dev.howangryami.telotrope.ai`,
`howangryami.telotrope.ai`), and CloudFront needs a certificate for it:

```
scripts/request-certificate.sh dev.howangryami.telotrope.ai
```

It requests the certificate (or reuses one already requested), prints a `CNAME` record to add at
Porkbun (*Domain Management → DNS*), waits until AWS has seen it, and prints the certificate's
identifier. Put that in [samconfig.toml](samconfig.toml) as the stack's `PageCertificateArn`.
**Keep the record at Porkbun:** AWS uses it again to renew the certificate. If the wait runs out
(40 minutes), run the script again once the record is in.

## 5. Deploy each stack the first time

```
scripts/deploy.sh dev
scripts/deploy.sh public      # only from main, committed and pushed; asks before applying
```

The first run of each creates its stack (a few minutes; CloudFront takes longest). At the end it
prints the page's address and, while the domain doesn't point there yet, the record to add at
Porkbun: `CNAME` from the domain's name (e.g. `dev.howangryami`) to the stack's CloudFront
address. [OPERATING.md](OPERATING.md) explains each step `deploy.sh` takes.

SAM creates a bucket of its own (`aws-sam-cli-managed-default-...`) to upload the code through.

## 6. Sign up

Open the stack's page and click **Sign in**. Cognito's own page opens. Choose *Sign up*, use your
email address, and enter the code Cognito emails you. Each stack has its own accounts: signing up
on one doesn't sign you up on the other.

## 7. Costs, and tearing it down

While idle, this should cost very little: the Secrets Manager secret is billed monthly (about
$0.40 at the time of writing; not re-checked), and everything else is billed per use. Your budget
from step 1 will warn you if that's wrong.

To delete everything:

```
stage=dev    # or public
aws s3 rm "s3://timeline-uploads-$stage-$(aws sts get-caller-identity --query Account --output text)" --recursive
aws s3 rm "s3://$(aws cloudformation describe-stacks --stack-name timeline-$stage --query "Stacks[0].Outputs[?OutputKey=='PageBucketName'].OutputValue" --output text)" --recursive
sam delete --stack-name timeline-$stage
```

The buckets must be emptied first; deleting a stack with a non-empty bucket fails. Deleting removes
your uploaded data and flags for good.

AWS's documentation says a deleted secret is kept for a recovery period, during which its name
can't be reused. If you plan to deploy again soon, delete it for good after `sam delete`:
`aws secretsmanager delete-secret --secret-id timeline-flag-handle-key-$stage --force-delete-without-recovery`.
