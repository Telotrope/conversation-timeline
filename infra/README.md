# Deploying to AWS: first-time walkthrough

This deploys the backend ([template.yaml](template.yaml)) to your own AWS account and points the
page, still served from your machine, at it. It's written for someone who hasn't used AWS
before. The design is in the migration plan's §V2e
([docs/plans/2026-09-09-rust-aws-backend-migration.md](../docs/plans/2026-09-09-rust-aws-backend-migration.md)).

Terms used below:

- **Region**: the AWS data-center location everything is created in, e.g. `us-east-1`.
- **Stack**: the set of resources AWS creates from the template, created and deleted as one.
- **IAM**: AWS's users-and-permissions system.

Nothing here has been run against a real account yet. Where a step's outcome is unknown, it says
so; the checks in step 8 exist to find out.

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
  step 4 with it.)
- **cargo-lambda and Zig**, for building: see [backend/README.md](../backend/README.md). On this
  machine both are already installed; Zig lives in `~/.local/opt/zig`.

Make sure `~/.local/bin` is on your `PATH`, then check: `aws --version` and `sam --version`.

## 3. Connect the tools to your account

```
aws configure sso          # your Identity Center start URL and region; name the profile "timeline"
export AWS_PROFILE=timeline AWS_REGION=us-east-1   # or your region; in every new terminal
aws sso login
aws sts get-caller-identity   # should print your 12-digit account number
```

## 4. Build and check

```
cd backend
PATH="$HOME/.local/opt/zig:$PATH" cargo lambda build --release --arm64 -p timeline-api
ls target/lambda/timeline-api/bootstrap target/lambda/process_upload/bootstrap
cd ..
scripts/check-template.sh          # expect: "... is a valid SAM Template"
```

The build makes both Lambdas: the API, and the function S3 starts when an upload lands.

## 5. Deploy

```
cd infra
sam deploy --guided --stack-name timeline-dev
```

The scripts below assume the stack is called `timeline-<stage>`, so keep that name. Answers to its
questions:

- **Region**: the one you chose. **Stage**: `dev`. **FrontendOrigin**: `http://localhost:8000`
  (where you'll serve the page; keep the default).
- "Confirm changes before deploy": **y**. It then lists everything it will create and waits.
- "Allow SAM CLI IAM role creation": **y** (each function needs its own permissions).
- It may ask whether each function "may not have authorization defined": **y**. The API's routes
  require a Cognito login through the API's default setting; the question is about the function
  itself.
- "Save arguments to configuration file": **y**. It writes `samconfig.toml` here, so later
  deployments are just `sam deploy`. It holds no secrets.

SAM also creates a bucket of its own (`aws-sam-cli-managed-default-...`) to upload the code
through. Creating everything takes a few minutes. At the end it prints the stack's outputs:
`ApiUrl`, `CognitoDomain`, `UserPoolClientId` and others.

If it fails, the message names the resource and the reason. Copy it into our next conversation;
that's deployment check D1.

## 6. Point the page at the deployment

```
scripts/write-deploy-config.sh dev     # writes frontend/deploy-configs/dev.json
python3 -m http.server 8000            # from the repository's top folder
```

Open <http://localhost:8000/timeline.html?deploy=dev>. The page remembers the choice; to go back
to local development, open `timeline.html?deploy=` once.

Click **Sign in**. Cognito's own page opens. Choose *Sign up*, use your email address, and enter
the code Cognito emails you. You come back to the page signed in. Then upload your export as
usual.

## 7. A test login from the command line (dev stage only)

```
scripts/aws-dev-token.sh you@example.com      # asks for the password you signed up with
TOKEN=$(scripts/aws-dev-token.sh you@example.com)
API=<ApiUrl from step 5>
curl -H "Authorization: Bearer $TOKEN" "$API/conversations"
```

## 8. The checks only a deployment can do

These are the plan's checks D1–D9. Note what you see (copy the output) and bring it to our next
conversation; I'll record it in `docs/analysis/`.

| # | What to check | How |
|---|---|---|
| D1 | AWS accepted the template | step 5 finished without errors |
| D2 | The browser's permission check (CORS) works from your page only | `curl -i -X OPTIONS "$API/conversations" -H "Origin: http://localhost:8000" -H "Access-Control-Request-Method: GET" -H "Access-Control-Request-Headers: authorization"` should answer 204 with an `access-control-allow-origin` header. Repeat with `-H "Origin: http://example.com"`: no such header. Then the page itself works (step 6). |
| D3 | Real Cognito sign-in works | step 6's sign-in, and step 7's script |
| D4 | The API refuses bad logins | `curl -i "$API/conversations"` (no token) and with `-H "Authorization: Bearer nonsense"`: both 401 |
| D5 | A ~60 MB export is processed within the limits | upload your real export with the scan box ticked; then `sam logs --stack-name timeline-dev -n ProcessUploadFunction` and `-n ApiFunction`: each call's `REPORT` line shows `Duration` and `Max Memory Used` |
| D6 | The flag-handle secret reached the API | in the Review tab, tick a flag: the page says it saved |
| D7 | The real tables match what the tests assume | the whole page flow works; the tables are visible in the DynamoDB console |
| D8 | Real AWS events match the sample events the tests use | **not ready**: nothing in the code records a real event yet; see the plan's C24 |
| D9 | Start-up time, including downloading Cognito's keys | the `REPORT` lines of a function's first call show `Init Duration` |

`sam logs ... --tail` follows the logs live.

## 9. Costs, and tearing it down

While idle, this should cost very little: the Secrets Manager secret is billed monthly (about
$0.40 at the time of writing; not re-checked), and everything else is billed per use. Your budget
from step 1 will warn you if that's wrong.

To delete everything:

```
aws s3 rm "s3://timeline-uploads-dev-$(aws sts get-caller-identity --query Account --output text)" --recursive
sam delete --stack-name timeline-dev
```

The bucket must be emptied first; deleting a stack with a non-empty bucket fails. Deleting removes
your uploaded data and flags for good.

AWS's documentation says a deleted secret is kept for a recovery period, during which its name
can't be reused. If you plan to deploy again soon, delete it for good after `sam delete`:
`aws secretsmanager delete-secret --secret-id timeline-flag-handle-key-dev --force-delete-without-recovery`.
