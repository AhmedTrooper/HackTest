import { createFileRoute } from "@tanstack/react-router";
import {
	AlertCircle,
	CheckCircle,
	ChevronLeft,
	ChevronRight,
	Download,
	File,
	Filter,
	HardDrive,
	Plus,
	RefreshCw,
	Search,
	Settings,
	Tag,
	Upload,
} from "lucide-react";
import type React from "react";
import { useEffect, useState, useCallback } from "react";

export const Route = createFileRoute("/")({
	component: SubmissionsDashboard,
});

interface UploadedFile {
	file_id: string;
	file_name: string;
	mime_type: string;
	size_bytes: number;
}

interface Submission {
	id: string;
	title: string;
	description: string;
	category: string;
	priority: string;
	tags: string[];
	file_info?: UploadedFile;
	created_at: string;
}

interface ValidationError {
	field: string;
	message: string;
}

interface ErrorResponse {
	error: string;
	details: ValidationError[];
}

const CATEGORIES = ["Bug Report", "Feature Request", "Feedback", "Other"];
const PRIORITIES = ["Low", "Medium", "High"];

function SubmissionsDashboard() {
	// State for backend connection
	const [backendUrl, setBackendUrl] = useState(() => {
		if (typeof window !== "undefined") {
			return (
				localStorage.getItem("hack_backend_url") || "http://localhost:8080"
			);
		}
		return "http://localhost:8080";
	});
	const [showSettings, setShowSettings] = useState(false);
	const [connectionStatus, setConnectionStatus] = useState<
		"unchecked" | "ok" | "failed"
	>("unchecked");

	// Form states
	const [title, setTitle] = useState("");
	const [description, setDescription] = useState("");
	const [category, setCategory] = useState("Feedback");
	const [priority, setPriority] = useState("Medium");
	const [tagsInput, setTagsInput] = useState("");

	// File upload states
	const [selectedFile, setSelectedFile] = useState<File | null>(null);
	const [isUploading, setIsUploading] = useState(false);
	const [uploadedFileMeta, setUploadedFileMeta] = useState<UploadedFile | null>(
		null,
	);
	const [uploadProgress, setUploadProgress] = useState(0);

	// Feedback states
	const [formErrors, setFormErrors] = useState<ValidationError[]>([]);
	const [generalError, setGeneralError] = useState<string | null>(null);
	const [successMessage, setSuccessMessage] = useState<string | null>(null);
	const [isSubmitting, setIsSubmitting] = useState(false);

	// Submissions list states
	const [submissions, setSubmissions] = useState<Submission[]>([]);
	const [totalSubmissions, setTotalSubmissions] = useState(0);
	const [isLoadingList, setIsLoadingList] = useState(false);

	// Pagination & Filtering
	const [offset, setOffset] = useState(0);
	const [limit, setLimit] = useState(5);
	const [searchFilter, setSearchFilter] = useState("");
	const [categoryFilter, setCategoryFilter] = useState("");
	const [priorityFilter, setPriorityFilter] = useState("");

	// Check backend health
	const checkHealth = useCallback(async (urlToCheck = backendUrl) => {
		try {
			const response = await fetch(`${urlToCheck}/health`);
			const data = await response.json();
			if (data && data.status === "ok") {
				setConnectionStatus("ok");
			} else {
				setConnectionStatus("failed");
			}
		} catch (_e) {
			setConnectionStatus("failed");
		}
	}, [backendUrl]);

	// Effect to save backend URL and verify health
	useEffect(() => {
		localStorage.setItem("hack_backend_url", backendUrl);
		checkHealth();
	}, [backendUrl, checkHealth]);

	// Fetch submissions list (with query parameters)
	const fetchSubmissions = useCallback(async () => {
		setIsLoadingList(true);
		try {
			const params = new URLSearchParams();
			params.append("offset", offset.toString());
			params.append("limit", limit.toString());
			if (searchFilter) params.append("search", searchFilter);
			if (categoryFilter) params.append("category", categoryFilter);
			if (priorityFilter) params.append("priority", priorityFilter);

			const response = await fetch(
				`${backendUrl}/api/submissions?${params.toString()}`,
			);
			if (!response.ok) {
				throw new Error("Failed to retrieve list");
			}
			const data = await response.json();
			setSubmissions(data.items || []);
			setTotalSubmissions(data.total || 0);
		} catch (e: any) {
			console.error(e);
		} finally {
			setIsLoadingList(false);
		}
	}, [backendUrl, offset, limit, searchFilter, categoryFilter, priorityFilter]);

	// Refresh submission list on filter/page changes
	useEffect(() => {
		fetchSubmissions();
	}, [fetchSubmissions]);

	// Handle file select & immediate upload (up to 10MB limit checking)
	const handleFileChange = async (e: React.ChangeEvent<HTMLInputElement>) => {
		const file = e.target.files?.[0];
		if (!file) return;

		// 10MB check in client
		const maxSize = 10 * 1024 * 1024; // 10MB
		if (file.size > maxSize) {
			setGeneralError("File is too large. Max size is 10MB.");
			setSelectedFile(null);
			return;
		}

		setGeneralError(null);
		setSelectedFile(file);
		setIsUploading(true);
		setUploadProgress(20);

		try {
			const formData = new FormData();
			formData.append("file", file);

			setUploadProgress(50);
			const response = await fetch(`${backendUrl}/api/upload`, {
				method: "POST",
				body: formData,
			});

			setUploadProgress(80);
			if (!response.ok) {
				const errData: ErrorResponse = await response.json();
				throw new Error(errData.error || "Failed to upload file");
			}

			const fileMeta: UploadedFile = await response.json();
			setUploadedFileMeta(fileMeta);
			setUploadProgress(100);
			setSuccessMessage(`File "${file.name}" uploaded successfully!`);
			setTimeout(() => setSuccessMessage(null), 4000);
		} catch (err: any) {
			setGeneralError(err.message || "Network error during file upload");
			setSelectedFile(null);
		} finally {
			setIsUploading(false);
		}
	};

	// Handle form submission
	const handleSubmit = async (e: React.FormEvent) => {
		e.preventDefault();
		setIsSubmitting(true);
		setFormErrors([]);
		setGeneralError(null);
		setSuccessMessage(null);

		// Form tags parsing
		const tags = tagsInput
			.split(",")
			.map((t) => t.trim())
			.filter((t) => t.length > 0);

		// Build JSON payload
		const payload = {
			title: title ? title : null, // send null for empty string to test backend guardrails
			description: description ? description : null,
			category: category,
			priority: priority,
			tags: tags.length > 0 ? tags : null,
			file_id: uploadedFileMeta ? uploadedFileMeta.file_id : null,
		};

		try {
			const response = await fetch(`${backendUrl}/api/submissions`, {
				method: "POST",
				headers: {
					"Content-Type": "application/json",
				},
				body: JSON.stringify(payload),
			});

			if (response.ok) {
				// Success
				const _newSubmission: Submission = await response.json();
				setSuccessMessage("Submission added successfully!");

				// Reset form
				setTitle("");
				setDescription("");
				setCategory("Feedback");
				setPriority("Medium");
				setTagsInput("");
				setSelectedFile(null);
				setUploadedFileMeta(null);
				setUploadProgress(0);

				// Refresh list
				setOffset(0);
				fetchSubmissions();

				setTimeout(() => setSuccessMessage(null), 5000);
			} else {
				// Rejection / validation failure
				const errData: ErrorResponse = await response.json();
				setGeneralError(errData.error || "Validation failed");
				if (errData.details && Array.isArray(errData.details)) {
					setFormErrors(errData.details);
				}
			}
		} catch (err: any) {
			setGeneralError(
				err.message ||
					"Failed to submit form. Please verify backend connection.",
			);
		} finally {
			setIsSubmitting(false);
		}
	};

	const getFieldError = (fieldName: string) => {
		const error = formErrors.find((e) => e.field === fieldName);
		return error ? error.message : null;
	};

	const getPriorityColor = (prio: string) => {
		switch (prio) {
			case "High":
				return "bg-rose-500/10 text-rose-600 border border-rose-500/20";
			case "Medium":
				return "bg-amber-500/10 text-amber-600 border border-amber-500/20";
			case "Low":
				return "bg-emerald-500/10 text-emerald-600 border border-emerald-500/20";
			default:
				return "bg-slate-500/10 text-slate-600 border border-slate-500/20";
		}
	};

	const getCategoryColor = (cat: string) => {
		switch (cat) {
			case "Bug Report":
				return "bg-red-500/10 text-red-600 border border-red-500/20";
			case "Feature Request":
				return "bg-sky-500/10 text-sky-600 border border-sky-500/20";
			case "Feedback":
				return "bg-teal-500/10 text-teal-600 border border-teal-500/20";
			default:
				return "bg-purple-500/10 text-purple-600 border border-purple-500/20";
		}
	};

	const formatBytes = (bytes: number) => {
		if (bytes === 0) return "0 Bytes";
		const k = 1024;
		const sizes = ["Bytes", "KB", "MB"];
		const i = Math.floor(Math.log(bytes) / Math.log(k));
		return `${parseFloat((bytes / k ** i).toFixed(2))} ${sizes[i]}`;
	};

	return (
		<main className="page-wrap px-4 py-8">
			{/* Top Banner */}
			<section className="island-shell rise-in relative overflow-hidden rounded-[2rem] px-6 py-8 sm:px-10 sm:py-10 mb-8">
				<div className="pointer-events-none absolute -left-20 -top-24 h-56 w-56 rounded-full bg-[radial-gradient(circle,rgba(79,184,178,0.24),transparent_66%)]" />
				<div className="pointer-events-none absolute -bottom-20 -right-20 h-56 w-56 rounded-full bg-[radial-gradient(circle,rgba(47,106,74,0.12),transparent_66%)]" />

				<div className="flex flex-col md:flex-row md:items-center md:justify-between gap-4">
					<div>
						<p className="island-kicker mb-2">Hackathon Template Ready</p>
						<h1 className="display-title text-3xl sm:text-5xl font-bold tracking-tight text-[var(--sea-ink)] leading-none">
							Deploy & Verification Hub
						</h1>
						<p className="demo-muted mt-2 text-sm max-w-xl">
							Use this dashboard to verify if your cloud hosting behaves
							correctly under structured payloads, offset pagination, enums, and
							10MB multipart file uploads.
						</p>
					</div>

					<button
						type="button"
						onClick={() => setShowSettings(!showSettings)}
						className="flex items-center gap-2 self-start md:self-auto rounded-full border border-[rgba(23,58,64,0.15)] bg-white/60 dark:bg-black/20 px-4 py-2.5 text-xs font-semibold text-[var(--sea-ink)] hover:bg-white/90 transition shadow-sm"
					>
						<Settings
							className={`h-4 w-4 transition-transform ${showSettings ? "rotate-90" : ""}`}
						/>
						Backend Connection Config
					</button>
				</div>

				{/* Backend Settings Panel */}
				{showSettings && (
					<div className="mt-6 border-t border-[var(--line)] pt-6 fade-in">
						<div className="grid md:grid-cols-3 gap-4 items-end">
							<div className="md:col-span-2">
								<label htmlFor="backend-url-input" className="block text-xs font-bold text-[var(--sea-ink-soft)] uppercase tracking-wider mb-2">
									Backend API Root URL
								</label>
								<div className="relative">
									<input
										id="backend-url-input"
										type="text"
										value={backendUrl}
										onChange={(e) => setBackendUrl(e.target.value)}
										className="w-full pl-3 pr-24 py-2 text-sm rounded-lg border border-[var(--line)] bg-white/70 dark:bg-black/10 text-[var(--sea-ink)] focus:outline-none focus:ring-2 focus:ring-[var(--lagoon)]"
										placeholder="e.g. http://localhost:8080"
									/>
									<div className="absolute right-1 top-1">
										<button
											type="button"
											onClick={() => checkHealth()}
											className="px-3 py-1 rounded bg-[var(--lagoon-deep)] text-white text-xs font-medium hover:bg-[var(--lagoon)] transition flex items-center gap-1.5"
										>
											<RefreshCw className="h-3 w-3" />
											Test Link
										</button>
									</div>
								</div>
							</div>
							<div>
								<span className="block text-xs font-bold text-[var(--sea-ink-soft)] uppercase tracking-wider mb-2">
									Link Status
								</span>
								<div className="flex items-center gap-2 h-10 px-3 rounded-lg bg-white/40 dark:bg-black/5 border border-[var(--line)] text-xs font-medium">
									{connectionStatus === "ok" && (
										<>
											<span className="h-2 w-2 rounded-full bg-emerald-500 animate-pulse" />
											<span className="text-emerald-600 dark:text-emerald-400">
												API Live (0.0.0.0 OK)
											</span>
										</>
									)}
									{connectionStatus === "failed" && (
										<>
											<span className="h-2 w-2 rounded-full bg-rose-500" />
											<span className="text-rose-600 dark:text-rose-400">
												Offline / CORS Blocked
											</span>
										</>
									)}
									{connectionStatus === "unchecked" && (
										<>
											<span className="h-2 w-2 rounded-full bg-amber-500" />
											<span className="text-amber-600 dark:text-amber-400">
												Checking connection...
											</span>
										</>
									)}
								</div>
							</div>
						</div>
					</div>
				)}
			</section>

			{/* Main Grid */}
			<div className="grid lg:grid-cols-12 gap-8">
				{/* Left Column: Submission Form */}
				<section className="lg:col-span-5 space-y-6">
					<div className="island-shell rounded-2xl p-6 relative">
						<h2 className="text-lg font-bold text-[var(--sea-ink)] mb-4 flex items-center gap-2 border-b border-[var(--line)] pb-3">
							<Plus className="h-5 w-5 text-[var(--lagoon-deep)]" />
							Submission Form
						</h2>

						{/* Error & Success Messages */}
						{generalError && (
							<div className="mb-4 p-3 rounded-lg bg-rose-500/10 border border-rose-500/20 text-rose-700 dark:text-rose-300 text-xs flex items-start gap-2">
								<AlertCircle className="h-4 w-4 flex-shrink-0 mt-0.5" />
								<div>
									<span className="font-semibold">Error:</span> {generalError}
								</div>
							</div>
						)}

						{successMessage && (
							<div className="mb-4 p-3 rounded-lg bg-emerald-500/10 border border-emerald-500/20 text-emerald-700 dark:text-emerald-300 text-xs flex items-start gap-2">
								<CheckCircle className="h-4 w-4 flex-shrink-0 mt-0.5" />
								<div>{successMessage}</div>
							</div>
						)}

						<form onSubmit={handleSubmit} className="space-y-4">
							{/* Title input */}
							<div>
								<label htmlFor="title-input" className="block text-xs font-bold text-[var(--sea-ink-soft)] uppercase tracking-wider mb-1.5">
									Title <span className="text-rose-500">*</span>
								</label>
								<input
									id="title-input"
									type="text"
									value={title}
									onChange={(e) => setTitle(e.target.value)}
									className={`w-full px-3 py-2 text-sm rounded-lg border bg-white/50 dark:bg-black/10 text-[var(--sea-ink)] focus:outline-none focus:ring-2 focus:ring-[var(--lagoon)] ${
										getFieldError("title")
											? "border-rose-500/60 focus:ring-rose-500"
											: "border-[var(--line)]"
									}`}
									placeholder="Submission or task name"
								/>
								{getFieldError("title") && (
									<p className="mt-1 text-xs text-rose-600 dark:text-rose-400 flex items-center gap-1 font-medium">
										<AlertCircle className="h-3.5 w-3.5" />
										{getFieldError("title")}
									</p>
								)}
							</div>

							{/* Description textarea */}
							<div>
								<label htmlFor="description-input" className="block text-xs font-bold text-[var(--sea-ink-soft)] uppercase tracking-wider mb-1.5">
									Description <span className="text-rose-500">*</span>
								</label>
								<textarea
									id="description-input"
									value={description}
									onChange={(e) => setDescription(e.target.value)}
									rows={3}
									className={`w-full px-3 py-2 text-sm rounded-lg border bg-white/50 dark:bg-black/10 text-[var(--sea-ink)] focus:outline-none focus:ring-2 focus:ring-[var(--lagoon)] ${
										getFieldError("description")
											? "border-rose-500/60 focus:ring-rose-500"
											: "border-[var(--line)]"
									}`}
									placeholder="Summarize the request details..."
								/>
								{getFieldError("description") && (
									<p className="mt-1 text-xs text-rose-600 dark:text-rose-400 flex items-center gap-1 font-medium">
										<AlertCircle className="h-3.5 w-3.5" />
										{getFieldError("description")}
									</p>
								)}
							</div>

							{/* Grid for Category & Priority */}
							<div className="grid grid-cols-2 gap-4">
								{/* Category select */}
								<div>
									<label htmlFor="category-select" className="block text-xs font-bold text-[var(--sea-ink-soft)] uppercase tracking-wider mb-1.5">
										Category
									</label>
									<select
										id="category-select"
										value={category}
										onChange={(e) => setCategory(e.target.value)}
										className="w-full px-2 py-2 text-sm rounded-lg border border-[var(--line)] bg-white/50 dark:bg-black/10 text-[var(--sea-ink)] focus:outline-none focus:ring-2 focus:ring-[var(--lagoon)]"
									>
										{CATEGORIES.map((cat) => (
											<option key={cat} value={cat}>
												{cat}
											</option>
										))}
									</select>
									{getFieldError("category") && (
										<p className="mt-1 text-xs text-rose-600 dark:text-rose-400 font-medium">
											{getFieldError("category")}
										</p>
									)}
								</div>

								{/* Priority select */}
								<div>
									<label htmlFor="priority-select" className="block text-xs font-bold text-[var(--sea-ink-soft)] uppercase tracking-wider mb-1.5">
										Priority
									</label>
									<select
										id="priority-select"
										value={priority}
										onChange={(e) => setPriority(e.target.value)}
										className="w-full px-2 py-2 text-sm rounded-lg border border-[var(--line)] bg-white/50 dark:bg-black/10 text-[var(--sea-ink)] focus:outline-none focus:ring-2 focus:ring-[var(--lagoon)]"
									>
										{PRIORITIES.map((prio) => (
											<option key={prio} value={prio}>
												{prio}
											</option>
										))}
									</select>
									{getFieldError("priority") && (
										<p className="mt-1 text-xs text-rose-600 dark:text-rose-400 font-medium">
											{getFieldError("priority")}
										</p>
									)}
								</div>
							</div>

							{/* Tags comma list */}
							<div>
								<label htmlFor="tags-input" className="block text-xs font-bold text-[var(--sea-ink-soft)] uppercase tracking-wider mb-1.5 flex items-center justify-between">
									<span>Tags</span>
									<span className="text-[10px] text-slate-400 lowercase italic">
										comma separated
									</span>
								</label>
								<div className="relative">
									<span className="absolute left-3 top-2.5 text-slate-400">
										<Tag className="h-4 w-4" />
									</span>
									<input
										id="tags-input"
										type="text"
										value={tagsInput}
										onChange={(e) => setTagsInput(e.target.value)}
										className="w-full pl-9 pr-3 py-2 text-sm rounded-lg border border-[var(--line)] bg-white/50 dark:bg-black/10 text-[var(--sea-ink)] focus:outline-none focus:ring-2 focus:ring-[var(--lagoon)]"
										placeholder="hackathon, test, cloud"
									/>
								</div>
								{getFieldError("tags") && (
									<p className="mt-1 text-xs text-rose-600 dark:text-rose-400 font-medium">
										{getFieldError("tags")}
									</p>
								)}
							</div>

							{/* File upload drag&drop zone */}
							<div className="border-2 border-dashed border-[var(--line)] rounded-xl p-4 bg-white/30 dark:bg-black/5 hover:bg-white/60 hover:border-[var(--lagoon)] transition relative overflow-hidden">
								<input
									type="file"
									id="file-upload"
									onChange={handleFileChange}
									className="absolute inset-0 w-full h-full opacity-0 cursor-pointer"
									disabled={isUploading}
								/>

								<div className="flex flex-col items-center justify-center text-center p-2">
									{!selectedFile ? (
										<>
											<Upload className="h-8 w-8 text-[var(--lagoon-deep)] mb-2" />
											<p className="text-sm font-semibold text-[var(--sea-ink)]">
												Drag & Drop or Click to Upload
											</p>
											<p className="text-xs text-[var(--sea-ink-soft)] mt-1">
												Any attachment file up to 10MB
											</p>
										</>
									) : (
										<div className="w-full flex items-center justify-between gap-3 text-left">
											<div className="flex items-center gap-2 min-w-0">
												<File className="h-6 w-6 text-[var(--lagoon-deep)] flex-shrink-0" />
												<div className="min-w-0">
													<p className="text-xs font-semibold text-[var(--sea-ink)] truncate">
														{selectedFile.name}
													</p>
													<p className="text-[10px] text-slate-400">
														{formatBytes(selectedFile.size)}
													</p>
												</div>
											</div>

											<div className="flex-shrink-0">
												{isUploading ? (
													<span className="text-[10px] bg-sky-500/10 text-sky-600 px-2 py-1 rounded font-medium animate-pulse">
														Uploading ({uploadProgress}%)
													</span>
												) : uploadedFileMeta ? (
													<span className="text-[10px] bg-emerald-500/10 text-emerald-600 px-2 py-1 rounded font-medium flex items-center gap-1">
														<CheckCircle className="h-3.5 w-3.5" />
														Ready
													</span>
												) : (
													<span className="text-[10px] bg-rose-500/10 text-rose-600 px-2 py-1 rounded font-medium">
														Failed
													</span>
												)}
											</div>
										</div>
									)}
								</div>

								{/* Progress bar */}
								{isUploading && (
									<div
										className="absolute bottom-0 left-0 h-1 bg-[var(--lagoon)] transition-all duration-300"
										style={{ width: `${uploadProgress}%` }}
									/>
								)}
							</div>
							{getFieldError("file_id") && (
								<p className="text-xs text-rose-600 dark:text-rose-400 flex items-center gap-1 font-medium">
									<AlertCircle className="h-3.5 w-3.5" />
									{getFieldError("file_id")}
								</p>
							)}

							{/* Submit button */}
							<button
								type="submit"
								disabled={
									isSubmitting || isUploading || connectionStatus === "failed"
								}
								className="w-full py-2.5 rounded-xl bg-[var(--sea-ink)] hover:bg-[var(--sea-ink-soft)] text-white font-bold text-sm shadow-sm transition disabled:opacity-50 flex items-center justify-center gap-2 cursor-pointer"
							>
								{isSubmitting ? (
									<>
										<RefreshCw className="h-4 w-4 animate-spin" />
										Submitting payload...
									</>
								) : (
									<>
										<Plus className="h-4 w-4" />
										Add Submission
									</>
								)}
							</button>
						</form>
					</div>
				</section>

				{/* Right Column: Query Filters and Offset Pagination List */}
				<section className="lg:col-span-7 space-y-6">
					<div className="island-shell rounded-2xl p-6">
						{/* Header & Refresh */}
						<div className="flex items-center justify-between border-b border-[var(--line)] pb-3 mb-4">
							<h2 className="text-lg font-bold text-[var(--sea-ink)] flex items-center gap-2">
								<HardDrive className="h-5 w-5 text-[var(--lagoon-deep)]" />
								Submitted Records
							</h2>
							<button
								type="button"
								onClick={fetchSubmissions}
								disabled={isLoadingList}
								className="p-1.5 rounded-lg border border-[var(--line)] bg-white/50 dark:bg-black/10 text-[var(--sea-ink)] hover:bg-white/80 transition flex items-center justify-center disabled:opacity-50"
								title="Refresh Records"
							>
								<RefreshCw
									className={`h-4 w-4 ${isLoadingList ? "animate-spin" : ""}`}
								/>
							</button>
						</div>

						{/* Filter grid */}
						<div className="grid md:grid-cols-3 gap-3 mb-6">
							{/* Search filter */}
							<div className="relative">
								<Search className="absolute left-2.5 top-2.5 h-4 w-4 text-slate-400" />
								<input
									type="text"
									value={searchFilter}
									onChange={(e) => {
										setSearchFilter(e.target.value);
										setOffset(0); // Reset to page 1 on filter
									}}
									className="w-full pl-8 pr-3 py-1.5 text-xs rounded-lg border border-[var(--line)] bg-white/40 dark:bg-black/10 text-[var(--sea-ink)] focus:outline-none focus:ring-1 focus:ring-[var(--lagoon)]"
									placeholder="Search title/tag/desc..."
								/>
							</div>

							{/* Category filter */}
							<div className="relative">
								<Filter className="absolute left-2.5 top-2.5 h-3.5 w-3.5 text-slate-400" />
								<select
									value={categoryFilter}
									onChange={(e) => {
										setCategoryFilter(e.target.value);
										setOffset(0); // Reset to page 1
									}}
									className="w-full pl-8 pr-2 py-1.5 text-xs rounded-lg border border-[var(--line)] bg-white/40 dark:bg-black/10 text-[var(--sea-ink)] focus:outline-none focus:ring-1 focus:ring-[var(--lagoon)] appearance-none"
								>
									<option value="">All Categories</option>
									{CATEGORIES.map((cat) => (
										<option key={cat} value={cat}>
											{cat}
										</option>
									))}
								</select>
							</div>

							{/* Priority filter */}
							<div className="relative">
								<Filter className="absolute left-2.5 top-2.5 h-3.5 w-3.5 text-slate-400" />
								<select
									value={priorityFilter}
									onChange={(e) => {
										setPriorityFilter(e.target.value);
										setOffset(0); // Reset to page 1
									}}
									className="w-full pl-8 pr-2 py-1.5 text-xs rounded-lg border border-[var(--line)] bg-white/40 dark:bg-black/10 text-[var(--sea-ink)] focus:outline-none focus:ring-1 focus:ring-[var(--lagoon)] appearance-none"
								>
									<option value="">All Priorities</option>
									{PRIORITIES.map((prio) => (
										<option key={prio} value={prio}>
											{prio}
										</option>
									))}
								</select>
							</div>
						</div>

						{/* List entries */}
						{isLoadingList ? (
							<div className="flex flex-col items-center justify-center py-12 gap-2 text-slate-400 text-sm">
								<RefreshCw className="h-8 w-8 animate-spin text-[var(--lagoon-deep)]" />
								<span>Fetching data from {backendUrl}...</span>
							</div>
						) : submissions.length === 0 ? (
							<div className="text-center py-12 border border-dashed border-[var(--line)] rounded-xl bg-white/10 dark:bg-black/5 text-slate-400">
								<File className="h-10 w-10 mx-auto text-slate-300 mb-2" />
								<p className="text-sm font-semibold">No submissions found</p>
								<p className="text-xs mt-1">
									Submit the form or change filters to load data.
								</p>
							</div>
						) : (
							<div className="space-y-4">
								{submissions.map((sub) => (
									<article
										key={sub.id}
										className="p-4 rounded-xl border border-[var(--line)] bg-white/40 dark:bg-black/15 shadow-sm transition hover:border-[var(--lagoon-deep)] hover:-translate-y-0.5"
									>
										<div className="flex flex-wrap items-start justify-between gap-2 mb-2">
											<h3 className="font-bold text-sm sm:text-base text-[var(--sea-ink)]">
												{sub.title}
											</h3>

											<div className="flex items-center gap-1.5">
												<span
													className={`text-[10px] px-2 py-0.5 rounded-full font-bold uppercase tracking-wider ${getCategoryColor(sub.category)}`}
												>
													{sub.category}
												</span>
												<span
													className={`text-[10px] px-2 py-0.5 rounded-full font-bold uppercase tracking-wider ${getPriorityColor(sub.priority)}`}
												>
													{sub.priority}
												</span>
											</div>
										</div>

										<p className="text-xs text-[var(--sea-ink-soft)] leading-relaxed mb-3 whitespace-pre-wrap">
											{sub.description}
										</p>

										{/* Tags row */}
										{sub.tags && sub.tags.length > 0 && (
											<div className="flex flex-wrap gap-1.5 mb-3">
												{sub.tags.map((t) => (
													<span
														key={t}
														className="text-[10px] bg-slate-100 dark:bg-slate-800 text-slate-500 px-2 py-0.5 rounded flex items-center gap-1"
													>
														<Tag className="h-3 w-3" />
														{t}
													</span>
												))}
											</div>
										)}

										{/* Attachment & Date footer */}
										<div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2 border-t border-[var(--line)] pt-3 text-[10px] text-slate-400">
											<div>
												Submitted at:{" "}
												{new Date(sub.created_at).toLocaleString()}
											</div>

											{sub.file_info && (
												<a
													href={`${backendUrl}/api/files/${sub.file_info.file_id}`}
													target="_blank"
													rel="noreferrer"
													className="inline-flex items-center gap-1 text-[var(--lagoon-deep)] hover:underline font-bold"
													title="Download attachment file"
												>
													<Download className="h-3 w-3" />
													<span>
														{sub.file_info.file_name} (
														{formatBytes(sub.file_info.size_bytes)})
													</span>
												</a>
											)}
										</div>
									</article>
								))}
							</div>
						)}

						{/* Offset Pagination Controls */}
						{totalSubmissions > 0 && (
							<div className="mt-6 pt-4 border-t border-[var(--line)] flex flex-wrap items-center justify-between gap-4 text-xs">
								{/* Limit selector */}
								<div className="flex items-center gap-2">
									<span className="text-slate-400">Show:</span>
									<select
										value={limit}
										onChange={(e) => {
											setLimit(Number(e.target.value));
											setOffset(0); // Reset to page 1
										}}
										className="border border-[var(--line)] bg-white/50 dark:bg-black/10 rounded px-1.5 py-1 text-xs"
									>
										{[5, 10, 15, 20].map((val) => (
											<option key={val} value={val}>
												{val}
											</option>
										))}
									</select>
									<span className="text-slate-400">
										of {totalSubmissions} items
									</span>
								</div>

								{/* Pagination page arrows */}
								<div className="flex items-center gap-1.5">
									<button
										type="button"
										onClick={() => setOffset(Math.max(0, offset - limit))}
										disabled={offset === 0 || isLoadingList}
										className="p-1.5 rounded border border-[var(--line)] hover:bg-white/80 transition disabled:opacity-30 cursor-pointer flex items-center justify-center"
										title="Previous Page"
									>
										<ChevronLeft className="h-4 w-4" />
									</button>

									<div className="font-bold min-w-16 text-center">
										Page {Math.floor(offset / limit) + 1} /{" "}
										{Math.ceil(totalSubmissions / limit)}
									</div>

									<button
										type="button"
										onClick={() => setOffset(offset + limit)}
										disabled={
											offset + limit >= totalSubmissions || isLoadingList
										}
										className="p-1.5 rounded border border-[var(--line)] hover:bg-white/80 transition disabled:opacity-30 cursor-pointer flex items-center justify-center"
										title="Next Page"
									>
										<ChevronRight className="h-4 w-4" />
									</button>
								</div>
							</div>
						)}
					</div>
				</section>
			</div>
		</main>
	);
}
