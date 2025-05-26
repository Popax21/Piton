Console.WriteLine("Hello, World!");
#pragma warning disable CS8600 // Intentional NRE for testing purposes
#pragma warning disable CS8602 // See above
// Test if NREs work properly since by default rust uses a signal handler altstack which can 
// be too small for the runtime to work with, see https://github.com/dotnet/runtime/issues/115438
// for the full details

// Piton currently includes a hacky fix to work around that

try {
	((object)null).GetType();
} catch (NullReferenceException) {
	Console.WriteLine("NRE caught!");
}
