Console.WriteLine("Hello, World!");
#pragma warning disable CS8600
#pragma warning disable CS8602
try {
	((object)null).GetType();
} catch (NullReferenceException) {
	Console.WriteLine("NRE caught!");
}
